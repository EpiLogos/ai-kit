//! Application-boundary read model for credential/provider status.
//!
//! `ProjectWorldReadModel` and `ContextResolution` disclose capabilities,
//! actions, actors and information horizon, but carry no credential field: a
//! consumer cannot yet ask "what credentials/providers does this world have,
//! and what is each one's status" and get a typed answer. This module is
//! that missing plumbing. It composes the already-proven deterministic
//! `credential::resolve_credential` algorithm into a disclosure shape built
//! the same way `project_world.rs` composes `ContextResolution` into
//! `ProjectWorldResource`: an owned, round-trippable projection over an
//! existing authoritative contract, without re-deriving anything.
//!
//! This type is deliberately freestanding rather than added as a field on
//! `ProjectWorldReadModel` itself: every direct field of that struct is
//! `pub`, so a new field forces every exhaustive struct-literal constructor
//! across the workspace -- including a `#[cfg(test)]` fixture in
//! `crates/aikit-tui/src/project_workspace.rs` -- to be updated in the same
//! change. Touching `aikit-tui` is out of scope for the task that introduced
//! this module (two other agents were concurrently editing that crate), so
//! `CredentialWorldDisclosure` is exposed at the crate boundary
//! (`aikit_core::{CredentialWorldDisclosure, disclose_credential_world, ..}`)
//! ready to be attached to `ProjectWorldReadModel` -- via a new field and a
//! `with_credential_world`-style builder, mirroring `with_versioned_world`
//! -- by whoever can safely touch that fixture next.
//!
//! `aikit-core` remains I/O-free: nothing here queries a live secret
//! provider. Callers (adapters, the TUI application boundary) gather the
//! `SecretProviderDescriptor` roster and the `SecretRequirement`s that apply
//! to a world, then hand them to `disclose_credential_world`, which is a
//! pure function over already-observed facts.
//!
//! Absence is modelled explicitly, never fabricated. Two independent places
//! can be genuinely unknown rather than genuinely empty:
//!
//! - the provider roster itself (`ProviderRosterKnowledge`): "no secret
//!   providers exist on this machine" is a real, confirmed negative
//!   (`Observed` with an empty `providers` Vec) and must never be confused
//!   with "the provider roster could not be enumerated" (`Unknown`);
//! - each credential's status (`CredentialStatusKnowledge`): a `Resolved`
//!   result whose `selected() == false` is a real, explained negative ("no
//!   eligible provider", backed by `provider_explanations`) and must never
//!   be confused with `Unresolved` ("we could not run resolution at all",
//!   for example because the roster itself was `Unknown`).
//!
//! No secret material ever reaches this module. `credential::SecretValue` is
//! neither `Serialize` nor `Clone`, and nothing declared here is typed to
//! hold one; only identity, provenance and status cross this boundary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::credential::{
    resolve_credential, CredentialProviderRejection, CredentialRef, CredentialResolution,
    CredentialResolutionRequest, ProviderResolutionExplanation, SecretMaterialisationClass,
    SecretProviderDescriptor, SecretProviderRef, SecretProviderTier, SecretRequirement,
    SecretRequirementRef,
};
use crate::resource::{CredentialCondition, ModelRouteSet};

pub const CREDENTIAL_WORLD_VERSION: &str = "aikit.credential-world/v1";

/// Whether the set of secret providers reachable by this AIKit world could be
/// established at all.
///
/// `Observed` with an empty `providers` Vec is a genuine, confirmed "no
/// providers" reading -- not a stand-in for missing data. `Unknown` is the
/// only representation of "we could not tell"; it is never inferred from an
/// empty collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ProviderRosterKnowledge {
    Observed {
        providers: Vec<SecretProviderDescriptor>,
    },
    Unknown {
        reason: String,
    },
}

impl ProviderRosterKnowledge {
    /// `Some` only when the roster was actually observed (possibly empty).
    /// `None` means the roster is `Unknown` -- a caller must not treat that
    /// as an empty list.
    pub fn providers(&self) -> Option<&[SecretProviderDescriptor]> {
        match self {
            Self::Observed { providers } => Some(providers),
            Self::Unknown { .. } => None,
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Self::Observed { .. })
    }
}

/// Owned, round-trippable disclosure of one provider's standing in a
/// resolution outcome. Derived from `ProviderResolutionExplanation`, whose
/// own fields never carry secret material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderResolutionDisclosure {
    pub provider_ref: SecretProviderRef,
    pub eligible: bool,
    pub rejection: Option<CredentialProviderRejection>,
    pub selected_materialisation: Option<SecretMaterialisationClass>,
    pub assurance: String,
    pub degradation: Option<String>,
    pub binding_provenance: String,
}

impl From<&ProviderResolutionExplanation> for ProviderResolutionDisclosure {
    fn from(value: &ProviderResolutionExplanation) -> Self {
        Self {
            provider_ref: value.provider_ref.clone(),
            eligible: value.eligible,
            rejection: value.rejection.clone(),
            selected_materialisation: value.selected_materialisation.clone(),
            assurance: value.assurance.clone(),
            degradation: value.degradation.clone(),
            binding_provenance: value.binding_provenance.clone(),
        }
    }
}

/// Owned, round-trippable projection of `CredentialResolution`.
///
/// `CredentialResolution` itself carries a `&'static str` version field and
/// deliberately does not derive `Deserialize` (it is meant to be produced
/// only by `resolve_credential`, never forged from arbitrary input). This
/// disclosure is the read-model-safe copy that crosses the serialization
/// boundary: same facts, owned `String`s, full round trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialResolutionDisclosure {
    pub version: String,
    pub requirement_ref: SecretRequirementRef,
    pub credential_ref: CredentialRef,
    pub consumer_ref: String,
    pub purpose: String,
    pub selected_provider_ref: Option<SecretProviderRef>,
    pub selected_provider_tier: Option<SecretProviderTier>,
    pub selected_materialisation: Option<SecretMaterialisationClass>,
    pub assurance: Option<String>,
    pub degradation: Option<String>,
    pub binding_provenance: Option<String>,
    pub provider_explanations: Vec<ProviderResolutionDisclosure>,
}

impl CredentialResolutionDisclosure {
    /// A real, explained "no" -- not "we didn't check". `provider_explanations`
    /// names exactly why each considered provider was or was not eligible.
    pub fn selected(&self) -> bool {
        self.selected_provider_ref.is_some()
    }
}

impl From<&CredentialResolution> for CredentialResolutionDisclosure {
    fn from(value: &CredentialResolution) -> Self {
        Self {
            version: value.version.to_string(),
            requirement_ref: value.requirement_ref.clone(),
            credential_ref: value.credential_ref.clone(),
            consumer_ref: value.consumer_ref.clone(),
            purpose: value.purpose.clone(),
            selected_provider_ref: value.selected_provider_ref.clone(),
            selected_provider_tier: value.selected_provider_tier,
            selected_materialisation: value.selected_materialisation.clone(),
            assurance: value.assurance.clone(),
            degradation: value.degradation.clone(),
            binding_provenance: value.binding_provenance.clone(),
            provider_explanations: value
                .provider_explanations
                .iter()
                .map(ProviderResolutionDisclosure::from)
                .collect(),
        }
    }
}

/// Per-credential status knowledge.
///
/// `Resolved` carries a full deterministic resolution outcome: its own
/// `selected() == false` is a real, explained negative. `Unresolved` covers
/// every case where resolution itself could not be attempted or failed
/// validation (for example because the provider roster is `Unknown`, or the
/// `SecretRequirement` itself was invalid) -- so a caller never mistakes
/// "not attempted" for "resolved to no".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CredentialStatusKnowledge {
    Resolved(CredentialResolutionDisclosure),
    Unresolved {
        requirement_ref: SecretRequirementRef,
        credential_ref: CredentialRef,
        reason: String,
    },
}

impl CredentialStatusKnowledge {
    pub fn requirement_ref(&self) -> &SecretRequirementRef {
        match self {
            Self::Resolved(resolution) => &resolution.requirement_ref,
            Self::Unresolved { requirement_ref, .. } => requirement_ref,
        }
    }

    pub fn credential_ref(&self) -> &CredentialRef {
        match self {
            Self::Resolved(resolution) => &resolution.credential_ref,
            Self::Unresolved { credential_ref, .. } => credential_ref,
        }
    }

    /// `true` only for a positively-resolved, selected credential. Every
    /// other case (`Resolved` with nothing selected, or `Unresolved`) is
    /// `false`, but only `Resolved { .. }` with `selected() == false` is a
    /// real negative; `Unresolved` is an open question, not a "no".
    pub fn is_selected(&self) -> bool {
        matches!(self, Self::Resolved(resolution) if resolution.selected())
    }
}

/// Application-boundary read model answering "what credentials/providers
/// does this world have, and what is each one's status" with typed values.
///
/// `credentials` is keyed by `SecretRequirementRef` (stable per requirement)
/// in a `BTreeMap` for deterministic ordering. Both this map and the
/// provider roster independently distinguish "there are none" from "we
/// could not tell" -- see `ProviderRosterKnowledge` and
/// `CredentialStatusKnowledge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialWorldDisclosure {
    pub version: String,
    pub providers: ProviderRosterKnowledge,
    pub credentials: BTreeMap<SecretRequirementRef, CredentialStatusKnowledge>,
}

impl CredentialWorldDisclosure {
    /// An honest "nothing was attempted" reading. Used as the default when a
    /// composition path (such as `disclose_project_world`) has no credential
    /// input wired in yet -- this must never be confused with a positive
    /// observation of zero providers/credentials.
    pub fn not_attempted(reason: impl Into<String>) -> Self {
        Self {
            version: CREDENTIAL_WORLD_VERSION.to_string(),
            providers: ProviderRosterKnowledge::Unknown {
                reason: reason.into(),
            },
            credentials: BTreeMap::new(),
        }
    }

    pub fn status(&self, requirement_ref: &SecretRequirementRef) -> Option<&CredentialStatusKnowledge> {
        self.credentials.get(requirement_ref)
    }

    /// `true` only when the provider roster and every credential status are
    /// positively known. `false` does not mean "broken" -- it names cases
    /// where a caller must render an unknown state rather than a real one.
    pub fn fully_observed(&self) -> bool {
        self.providers.is_known()
            && self
                .credentials
                .values()
                .all(|status| matches!(status, CredentialStatusKnowledge::Resolved(_)))
    }
}

impl Default for CredentialWorldDisclosure {
    fn default() -> Self {
        Self::not_attempted("credential disclosure was not attempted for this resolution")
    }
}

/// Compose a `CredentialWorldDisclosure` from an already-observed provider
/// roster and the `SecretRequirement`s that apply to this world.
///
/// This is a pure function: it performs no I/O and reuses the existing
/// deterministic `resolve_credential` algorithm unchanged. When the roster
/// is `Unknown`, every requirement is reported `Unresolved` rather than
/// resolved against an empty provider list -- an empty list is a real
/// negative only when the roster itself was genuinely observed.
pub fn disclose_credential_world(
    providers: ProviderRosterKnowledge,
    requirements: &[SecretRequirement],
    headless: bool,
    allow_from_env: bool,
) -> CredentialWorldDisclosure {
    let mut credentials = BTreeMap::new();

    for requirement in requirements {
        let status = match &providers {
            ProviderRosterKnowledge::Observed { providers } => {
                match resolve_credential(CredentialResolutionRequest {
                    requirement: requirement.clone(),
                    providers: providers.clone(),
                    headless,
                    allow_from_env,
                }) {
                    Ok(resolution) => {
                        CredentialStatusKnowledge::Resolved(CredentialResolutionDisclosure::from(&resolution))
                    }
                    Err(error) => CredentialStatusKnowledge::Unresolved {
                        requirement_ref: requirement.requirement_ref.clone(),
                        credential_ref: requirement.credential_ref.clone(),
                        reason: error.message().to_string(),
                    },
                }
            }
            ProviderRosterKnowledge::Unknown { reason } => CredentialStatusKnowledge::Unresolved {
                requirement_ref: requirement.requirement_ref.clone(),
                credential_ref: requirement.credential_ref.clone(),
                reason: format!("provider roster is unknown: {reason}"),
            },
        };
        credentials.insert(requirement.requirement_ref.clone(), status);
    }

    CredentialWorldDisclosure {
        version: CREDENTIAL_WORLD_VERSION.to_string(),
        providers,
        credentials,
    }
}

/// The consumer identity recorded on a requirement derived from Model routes:
/// the world's model-routing consumer, not any one Model or provider.
const MODEL_ROUTE_CREDENTIAL_CONSUMER: &str = "aikit:model-routes";

/// Derive the credential requirements a world declares, read straight from its
/// resolved Model routes.
///
/// This is the requirements half of the credential world and it invents
/// nothing. A requirement exists for exactly those providers whose resolved
/// routes declare a credential need (`Required` or `Satisfied`); a route that
/// needs none (`NotRequired`, e.g. a local Ollama serving) contributes
/// nothing. The credential's identity is read from the route itself — the
/// actual bound `binding_ref` when one is recorded, or the provider it belongs
/// to when no binding names it yet.
///
/// One requirement per provider: several catalogued Models reaching the same
/// provider share one credential, so they collapse to one requirement rather
/// than multiplying it. A `Satisfied` route's real bound identity is preferred
/// over the synthesized one if both are seen for a provider. Ordering is
/// deterministic, keyed by provider.
///
/// A route's credential condition is fixed by the catalogue's declared need
/// joined against recorded bindings; it does not depend on whether the route
/// was observed on this machine. So this derivation is a pure read over
/// already-resolved routes — the same property that lets `disclose_project_world`
/// stay I/O-free.
pub fn credential_requirements_for_model_routes(
    routes: &[ModelRouteSet],
) -> Vec<SecretRequirement> {
    // provider ref -> (credential_ref, purpose hint, identity is a real binding).
    let mut by_provider: BTreeMap<String, (CredentialRef, String, bool)> = BTreeMap::new();

    for set in routes {
        for route in &set.routes {
            let (hint, binding_ref) = match &route.credential {
                CredentialCondition::NotRequired => continue,
                CredentialCondition::Required { hint } => (hint, None),
                CredentialCondition::Satisfied { hint, binding_ref } => (hint, Some(binding_ref)),
            };
            let provider = route.provider.as_str();
            let bound = binding_ref.is_some();
            let credential_ref = match binding_ref {
                Some(binding_ref) => CredentialRef::new(binding_ref.clone()),
                None => CredentialRef::new(derived_credential_ref(provider)),
            };
            let Ok(credential_ref) = credential_ref else {
                continue;
            };

            match by_provider.get_mut(provider) {
                // A real bound identity supersedes a synthesized one; nothing
                // else about an already-seen provider changes.
                Some(existing) if bound && !existing.2 => {
                    existing.0 = credential_ref;
                    existing.2 = true;
                }
                Some(_) => {}
                None => {
                    by_provider
                        .insert(provider.to_string(), (credential_ref, hint.clone(), bound));
                }
            }
        }
    }

    by_provider
        .into_iter()
        .filter_map(|(_provider, (credential_ref, hint, _))| {
            let requirement_ref = SecretRequirementRef::new(format!(
                "secret-requirement:{}",
                credential_ref.as_str()
            ))
            .ok()?;
            Some(SecretRequirement {
                requirement_ref,
                credential_ref,
                consumer_ref: MODEL_ROUTE_CREDENTIAL_CONSUMER.to_string(),
                purpose: hint,
                permitted_materialisation: [
                    SecretMaterialisationClass::ProviderNativeLease,
                    SecretMaterialisationClass::ProcessEnv,
                ]
                .into_iter()
                .collect(),
            })
        })
        .collect()
}

/// The credential identity to use for a route whose provider needs a credential
/// but has no binding naming one yet. Keyed to the provider so it stays stable
/// and readable (`provider:openai` -> `credential:openai`); the exact ref does
/// not affect the outcome, which is a genuine "no binding" either way.
fn derived_credential_ref(provider: &str) -> String {
    let vendor = provider.strip_prefix("provider:").unwrap_or(provider);
    format!("credential:{vendor}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::SecretValue;

    fn requirement(id: &str) -> SecretRequirement {
        SecretRequirement {
            requirement_ref: SecretRequirementRef::new(format!("secret-requirement:{id}")).unwrap(),
            credential_ref: CredentialRef::new(format!("credential:{id}")).unwrap(),
            consumer_ref: "harness:pi".into(),
            purpose: "provider inference".into(),
            permitted_materialisation: [
                SecretMaterialisationClass::CredentialBroker,
                SecretMaterialisationClass::ProviderNativeLease,
                SecretMaterialisationClass::ProcessEnv,
            ]
            .into_iter()
            .collect(),
        }
    }

    fn keychain_provider(id: &str) -> SecretProviderDescriptor {
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new(id).unwrap(),
            provider_kind: id.into(),
            tier: SecretProviderTier::OsSecureStore,
            available: true,
            headless_capable: true,
            assurance: "os-keychain".into(),
            degradation: None,
            supported_credentials: [CredentialRef::new("credential:openai").unwrap()]
                .into_iter()
                .collect(),
            supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                .into_iter()
                .collect(),
            binding_provenance: format!("binding:{id}"),
            revision_or_lease_class: Some("revision:v1".into()),
        }
    }

    #[test]
    fn empty_provider_roster_is_a_real_negative_not_an_unknown() {
        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed { providers: vec![] },
            &[requirement("openai")],
            false,
            false,
        );

        assert!(disclosure.providers.is_known());
        assert_eq!(disclosure.providers.providers(), Some(&[][..]));

        let status = disclosure
            .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
            .unwrap();
        assert!(matches!(status, CredentialStatusKnowledge::Resolved(_)));
        assert!(!status.is_selected());
        assert!(disclosure.fully_observed());
    }

    #[test]
    fn unknown_provider_roster_never_looks_like_a_confirmed_absence() {
        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Unknown {
                reason: "provider enumeration is not wired up yet".into(),
            },
            &[requirement("openai")],
            false,
            false,
        );

        assert!(!disclosure.providers.is_known());
        assert_eq!(disclosure.providers.providers(), None);

        let status = disclosure
            .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
            .unwrap();
        assert!(matches!(status, CredentialStatusKnowledge::Unresolved { .. }));
        assert!(!status.is_selected());
        assert!(!disclosure.fully_observed());
    }

    #[test]
    fn a_caller_can_distinguish_no_providers_configured_from_status_unavailable() {
        let none_configured = disclose_credential_world(
            ProviderRosterKnowledge::Observed { providers: vec![] },
            &[requirement("openai")],
            false,
            false,
        );
        let status_unavailable = disclose_credential_world(
            ProviderRosterKnowledge::Unknown {
                reason: "keychain query timed out".into(),
            },
            &[requirement("openai")],
            false,
            false,
        );

        // Same requirement, same absence of a bound credential on the
        // surface -- but the two readings must never collapse into each
        // other. One is a confirmed "no"; the other is an open question.
        assert!(none_configured.fully_observed());
        assert!(!status_unavailable.fully_observed());
        assert_ne!(
            none_configured.providers.is_known(),
            status_unavailable.providers.is_known()
        );
        assert_ne!(
            std::mem::discriminant(
                none_configured
                    .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
                    .unwrap()
            ),
            std::mem::discriminant(
                status_unavailable
                    .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
                    .unwrap()
            )
        );
    }

    #[test]
    fn resolved_credential_selects_the_eligible_keychain_provider() {
        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed {
                providers: vec![keychain_provider("provider:keychain")],
            },
            &[SecretRequirement {
                requirement_ref: SecretRequirementRef::new("secret-requirement:openai").unwrap(),
                credential_ref: CredentialRef::new("credential:openai").unwrap(),
                consumer_ref: "harness:pi".into(),
                purpose: "provider inference".into(),
                permitted_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                    .into_iter()
                    .collect(),
            }],
            false,
            false,
        );

        let status = disclosure
            .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
            .unwrap();
        assert!(status.is_selected());
        match status {
            CredentialStatusKnowledge::Resolved(resolution) => {
                assert_eq!(
                    resolution.selected_provider_ref.as_ref().unwrap().as_str(),
                    "provider:keychain"
                );
            }
            other => panic!("expected a resolved status, got {other:?}"),
        }
    }

    #[test]
    fn invalid_requirement_is_unresolved_rather_than_silently_absent() {
        let mut broken = requirement("openai");
        broken.permitted_materialisation.clear();

        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed {
                providers: vec![keychain_provider("provider:keychain")],
            },
            &[broken],
            false,
            false,
        );

        let status = disclosure
            .status(&SecretRequirementRef::new("secret-requirement:openai").unwrap())
            .unwrap();
        assert!(matches!(status, CredentialStatusKnowledge::Unresolved { .. }));
    }

    #[test]
    fn credential_world_disclosure_round_trips_through_json() {
        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed {
                providers: vec![keychain_provider("provider:keychain")],
            },
            &[requirement("openai"), requirement("anthropic")],
            true,
            false,
        );

        let json = serde_json::to_string(&disclosure).unwrap();
        let restored: CredentialWorldDisclosure = serde_json::from_str(&json).unwrap();
        assert_eq!(disclosure, restored);
    }

    #[test]
    fn not_attempted_default_round_trips_through_json() {
        let disclosure = CredentialWorldDisclosure::default();
        let json = serde_json::to_string(&disclosure).unwrap();
        let restored: CredentialWorldDisclosure = serde_json::from_str(&json).unwrap();
        assert_eq!(disclosure, restored);
        assert!(!disclosure.providers.is_known());
        assert!(disclosure.credentials.is_empty());
    }

    #[test]
    fn credentials_are_ordered_deterministically_by_requirement_ref() {
        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed { providers: vec![] },
            &[requirement("zeta"), requirement("alpha"), requirement("mid")],
            false,
            false,
        );
        let refs: Vec<&str> = disclosure
            .credentials
            .keys()
            .map(SecretRequirementRef::as_str)
            .collect();
        assert_eq!(
            refs,
            vec![
                "secret-requirement:alpha",
                "secret-requirement:mid",
                "secret-requirement:zeta",
            ]
        );
    }

    #[test]
    fn credential_world_disclosure_never_carries_secret_material() {
        // The raw value never has anywhere to go: no field on any type in
        // this module is typed to hold a `SecretValue`, and `SecretValue`
        // itself is neither `Serialize` nor `Clone` (see credential.rs), so
        // this is a structural guarantee, not merely an observed one. This
        // test proves the observed half: even a full, richly-populated
        // disclosure never contains the raw text a real secret would carry.
        let raw_secret = "sk-fixture-DO-NOT-LEAK-9f3c";
        let secret = SecretValue::new(raw_secret).unwrap();
        assert_eq!(format!("{secret:?}"), "SecretValue(<redacted>)");

        let disclosure = disclose_credential_world(
            ProviderRosterKnowledge::Observed {
                providers: vec![keychain_provider("provider:keychain")],
            },
            &[requirement("openai")],
            true,
            false,
        );

        let json = serde_json::to_string(&disclosure).unwrap();
        assert!(!json.contains(raw_secret));
        assert!(json.contains("credential:openai"));
        assert!(json.contains("provider:keychain"));
    }

    use crate::resource::{
        ModelRoute, ModelRouteKind, ProviderRef, ResourceRef, RouteAvailability,
    };

    fn route(model: &str, provider: &str, credential: CredentialCondition) -> ModelRoute {
        ModelRoute {
            model: ResourceRef::parse(model).unwrap(),
            provider: ProviderRef::parse(provider).unwrap(),
            kind: ModelRouteKind::ProviderNative,
            provider_native_id: "native-id".into(),
            endpoint: None,
            availability: RouteAvailability::Unobserved {
                reason: "declared by the catalogue; no provider observed it".into(),
            },
            credential,
            provenance: Vec::new(),
        }
    }

    fn set_with(model: &str, routes: Vec<ModelRoute>) -> ModelRouteSet {
        let mut set = ModelRouteSet::new(ResourceRef::parse(model).unwrap());
        set.routes = routes;
        set
    }

    #[test]
    fn a_route_that_needs_no_credential_declares_no_requirement() {
        let routes = vec![set_with(
            "model:local",
            vec![route("model:local", "provider:ollama", CredentialCondition::NotRequired)],
        )];
        assert!(credential_requirements_for_model_routes(&routes).is_empty());
    }

    #[test]
    fn a_required_route_declares_a_requirement_keyed_to_its_provider() {
        let routes = vec![set_with(
            "model:hosted",
            vec![route(
                "model:hosted",
                "provider:openai",
                CredentialCondition::Required {
                    hint: "provider:openai inference credential".into(),
                },
            )],
        )];
        let requirements = credential_requirements_for_model_routes(&routes);
        assert_eq!(requirements.len(), 1);
        assert_eq!(requirements[0].credential_ref.as_str(), "credential:openai");
        assert_eq!(
            requirements[0].requirement_ref.as_str(),
            "secret-requirement:credential:openai"
        );
        assert_eq!(requirements[0].purpose, "provider:openai inference credential");
    }

    #[test]
    fn a_satisfied_route_carries_the_real_bound_credential_identity() {
        let routes = vec![set_with(
            "model:hosted",
            vec![route(
                "model:hosted",
                "provider:openai",
                CredentialCondition::Satisfied {
                    hint: "provider:openai inference credential".into(),
                    binding_ref: "credential:openai/api-key".into(),
                },
            )],
        )];
        let requirements = credential_requirements_for_model_routes(&routes);
        assert_eq!(requirements.len(), 1);
        // The real bound ref, not a synthesized `credential:openai` — a
        // requirement resolved against the roster must match what is actually
        // bound in the store, and only the binding_ref names that.
        assert_eq!(
            requirements[0].credential_ref.as_str(),
            "credential:openai/api-key"
        );
    }

    #[test]
    fn many_models_sharing_a_provider_collapse_to_one_requirement() {
        let routes = vec![
            set_with(
                "model:a",
                vec![route(
                    "model:a",
                    "provider:openai",
                    CredentialCondition::Required { hint: "openai".into() },
                )],
            ),
            set_with(
                "model:b",
                vec![route(
                    "model:b",
                    "provider:openai",
                    CredentialCondition::Required { hint: "openai".into() },
                )],
            ),
            set_with(
                "model:c",
                vec![route(
                    "model:c",
                    "provider:anthropic",
                    CredentialCondition::Required { hint: "anthropic".into() },
                )],
            ),
        ];
        let requirements = credential_requirements_for_model_routes(&routes);
        // openai and anthropic — one each, not one per model.
        assert_eq!(requirements.len(), 2);
        let refs: Vec<&str> = requirements
            .iter()
            .map(|r| r.credential_ref.as_str())
            .collect();
        assert_eq!(refs, vec!["credential:anthropic", "credential:openai"]);
    }

    #[test]
    fn a_bound_route_upgrades_a_providers_synthesized_identity() {
        // The same provider seen first as Required (synthesized ref) then as
        // Satisfied (real bound ref): the requirement carries the real one.
        let routes = vec![
            set_with(
                "model:a",
                vec![route(
                    "model:a",
                    "provider:openai",
                    CredentialCondition::Required { hint: "openai".into() },
                )],
            ),
            set_with(
                "model:b",
                vec![route(
                    "model:b",
                    "provider:openai",
                    CredentialCondition::Satisfied {
                        hint: "openai".into(),
                        binding_ref: "credential:openai/api-key".into(),
                    },
                )],
            ),
        ];
        let requirements = credential_requirements_for_model_routes(&routes);
        assert_eq!(requirements.len(), 1);
        assert_eq!(
            requirements[0].credential_ref.as_str(),
            "credential:openai/api-key"
        );
    }
}
