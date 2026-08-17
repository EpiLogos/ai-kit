use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{AikitError, Result};

pub const CREDENTIAL_RESOLUTION_VERSION: &str = "aikit.credential-resolution/v1";

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("invalid_credential_resolution", message)
}

macro_rules! semantic_ref {
    ($name:ident, $label:literal) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(invalid(concat!($label, " must not be empty")));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

semantic_ref!(CredentialRef, "CredentialRef");
semantic_ref!(SecretRequirementRef, "SecretRequirementRef");
semantic_ref!(SecretProviderRef, "SecretProviderRef");

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretMaterialisationClass {
    ProcessEnv,
    OneShotChildProcess,
    FdOrPipe,
    FileOrTmpfsMount,
    ProviderNativeLease,
    CredentialBroker,
    ShortLivedFederatedCredential,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SecretRequirement {
    pub requirement_ref: SecretRequirementRef,
    pub credential_ref: CredentialRef,
    pub consumer_ref: String,
    pub purpose: String,
    pub permitted_materialisation: BTreeSet<SecretMaterialisationClass>,
}

impl SecretRequirement {
    pub fn validate(&self) -> Result<()> {
        if self.consumer_ref.trim().is_empty() {
            return Err(invalid("secret requirement consumer_ref must not be empty"));
        }
        if self.purpose.trim().is_empty() {
            return Err(invalid("secret requirement purpose must not be empty"));
        }
        if self.permitted_materialisation.is_empty() {
            return Err(invalid(
                "secret requirement must permit at least one materialisation class",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SecretProviderTier {
    OsSecureStore,
    BrokeredSecureProvider,
    ExplicitEncryptedFallback,
    FederatedOrDynamic,
    ExplicitEnvironmentImport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SecretProviderDescriptor {
    pub provider_ref: SecretProviderRef,
    pub provider_kind: String,
    pub tier: SecretProviderTier,
    pub available: bool,
    pub headless_capable: bool,
    pub assurance: String,
    pub degradation: Option<String>,
    pub supported_credentials: BTreeSet<CredentialRef>,
    pub supported_materialisation: BTreeSet<SecretMaterialisationClass>,
    pub binding_provenance: String,
    pub revision_or_lease_class: Option<String>,
}

impl SecretProviderDescriptor {
    pub fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("provider_kind", self.provider_kind.as_str()),
            ("assurance", self.assurance.as_str()),
            ("binding_provenance", self.binding_provenance.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(invalid(format!("secret provider {label} must not be empty")));
            }
        }
        if self.supported_materialisation.is_empty() {
            return Err(invalid(
                "secret provider must advertise at least one materialisation class",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialResolutionRequest {
    pub requirement: SecretRequirement,
    pub providers: Vec<SecretProviderDescriptor>,
    pub headless: bool,
    /// Explicit operator intent equivalent to the established `--from-env` escape hatch.
    /// Environment import is never eligible merely because a matching variable exists.
    pub allow_from_env: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialProviderRejection {
    Unavailable,
    NotHeadlessCapable,
    CredentialNotBound,
    NoPermittedMaterialisation,
    EnvironmentImportNotExplicitlyAllowed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderResolutionExplanation {
    pub provider_ref: SecretProviderRef,
    pub eligible: bool,
    pub rejection: Option<CredentialProviderRejection>,
    pub selected_materialisation: Option<SecretMaterialisationClass>,
    pub assurance: String,
    pub degradation: Option<String>,
    pub binding_provenance: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CredentialResolution {
    pub version: &'static str,
    pub requirement_ref: SecretRequirementRef,
    pub credential_ref: CredentialRef,
    pub consumer_ref: String,
    pub purpose: String,
    pub selected_provider_ref: Option<SecretProviderRef>,
    pub selected_materialisation: Option<SecretMaterialisationClass>,
    pub assurance: Option<String>,
    pub degradation: Option<String>,
    pub binding_provenance: Option<String>,
    pub provider_explanations: Vec<ProviderResolutionExplanation>,
}

impl CredentialResolution {
    pub fn selected(&self) -> bool {
        self.selected_provider_ref.is_some()
    }
}

pub fn resolve_credential(request: CredentialResolutionRequest) -> Result<CredentialResolution> {
    request.requirement.validate()?;
    for provider in &request.providers {
        provider.validate()?;
    }

    let mut explanations = Vec::with_capacity(request.providers.len());
    let mut eligible = Vec::new();

    for provider in request.providers {
        let materialisation = request
            .requirement
            .permitted_materialisation
            .intersection(&provider.supported_materialisation)
            .next()
            .cloned();

        let rejection = if !provider.available {
            Some(CredentialProviderRejection::Unavailable)
        } else if request.headless && !provider.headless_capable {
            Some(CredentialProviderRejection::NotHeadlessCapable)
        } else if !provider
            .supported_credentials
            .contains(&request.requirement.credential_ref)
        {
            Some(CredentialProviderRejection::CredentialNotBound)
        } else if materialisation.is_none() {
            Some(CredentialProviderRejection::NoPermittedMaterialisation)
        } else if provider.tier == SecretProviderTier::ExplicitEnvironmentImport
            && !request.allow_from_env
        {
            Some(CredentialProviderRejection::EnvironmentImportNotExplicitlyAllowed)
        } else {
            None
        };

        explanations.push(ProviderResolutionExplanation {
            provider_ref: provider.provider_ref.clone(),
            eligible: rejection.is_none(),
            rejection: rejection.clone(),
            selected_materialisation: materialisation.clone(),
            assurance: provider.assurance.clone(),
            degradation: provider.degradation.clone(),
            binding_provenance: provider.binding_provenance.clone(),
        });

        if rejection.is_none() {
            eligible.push((provider, materialisation.expect("eligible provider has materialisation")));
        }
    }

    eligible.sort_by(|(a, a_class), (b, b_class)| {
        a.tier
            .cmp(&b.tier)
            .then_with(|| materialisation_rank(a_class).cmp(&materialisation_rank(b_class)))
            .then_with(|| a.provider_ref.cmp(&b.provider_ref))
    });

    let (selected_provider_ref, selected_materialisation, assurance, degradation, provenance) =
        match eligible.into_iter().next() {
            Some((provider, class)) => (
                Some(provider.provider_ref),
                Some(class),
                Some(provider.assurance),
                provider.degradation,
                Some(provider.binding_provenance),
            ),
            None => (None, None, None, None, None),
        };

    Ok(CredentialResolution {
        version: CREDENTIAL_RESOLUTION_VERSION,
        requirement_ref: request.requirement.requirement_ref,
        credential_ref: request.requirement.credential_ref,
        consumer_ref: request.requirement.consumer_ref,
        purpose: request.requirement.purpose,
        selected_provider_ref,
        selected_materialisation,
        assurance,
        degradation,
        binding_provenance: provenance,
        provider_explanations: explanations,
    })
}

fn materialisation_rank(class: &SecretMaterialisationClass) -> u8 {
    match class {
        SecretMaterialisationClass::CredentialBroker => 0,
        SecretMaterialisationClass::ShortLivedFederatedCredential => 1,
        SecretMaterialisationClass::ProviderNativeLease => 2,
        SecretMaterialisationClass::FdOrPipe => 3,
        SecretMaterialisationClass::OneShotChildProcess => 4,
        SecretMaterialisationClass::FileOrTmpfsMount => 5,
        SecretMaterialisationClass::ProcessEnv => 6,
    }
}

/// Safe binding state. Rotation/replacement changes provider state while `CredentialRef` stays stable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialBindingState {
    pub credential_ref: CredentialRef,
    pub provider_ref: SecretProviderRef,
    pub binding_provenance: String,
    pub revision_or_lease_class: Option<String>,
    pub expires_at: Option<String>,
    pub revoked: bool,
    pub metadata: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(value: &str) -> CredentialRef {
        CredentialRef::new(value).unwrap()
    }

    fn requirement() -> SecretRequirement {
        SecretRequirement {
            requirement_ref: SecretRequirementRef::new("secret-requirement:openai-api").unwrap(),
            credential_ref: credential("credential:openai/research"),
            consumer_ref: "harness:pi".into(),
            purpose: "provider inference".into(),
            permitted_materialisation: [
                SecretMaterialisationClass::CredentialBroker,
                SecretMaterialisationClass::ProcessEnv,
            ]
            .into_iter()
            .collect(),
        }
    }

    fn provider(
        id: &str,
        tier: SecretProviderTier,
        classes: impl IntoIterator<Item = SecretMaterialisationClass>,
    ) -> SecretProviderDescriptor {
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new(id).unwrap(),
            provider_kind: id.into(),
            tier,
            available: true,
            headless_capable: true,
            assurance: "test-assurance".into(),
            degradation: None,
            supported_credentials: [credential("credential:openai/research")]
                .into_iter()
                .collect(),
            supported_materialisation: classes.into_iter().collect(),
            binding_provenance: format!("binding:{id}"),
            revision_or_lease_class: Some("revision:v1".into()),
        }
    }

    #[test]
    fn keychain_first_then_rich_provider_then_explicit_encrypted_fallback() {
        let result = resolve_credential(CredentialResolutionRequest {
            requirement: requirement(),
            providers: vec![
                provider(
                    "provider:encrypted-fallback",
                    SecretProviderTier::ExplicitEncryptedFallback,
                    [SecretMaterialisationClass::ProcessEnv],
                ),
                provider(
                    "provider:varlock",
                    SecretProviderTier::BrokeredSecureProvider,
                    [SecretMaterialisationClass::CredentialBroker],
                ),
                provider(
                    "provider:keychain",
                    SecretProviderTier::OsSecureStore,
                    [SecretMaterialisationClass::ProcessEnv],
                ),
            ],
            headless: false,
            allow_from_env: false,
        })
        .unwrap();

        assert_eq!(
            result.selected_provider_ref.unwrap().as_str(),
            "provider:keychain"
        );
    }

    #[test]
    fn headless_environment_import_never_silently_downgrades() {
        let result = resolve_credential(CredentialResolutionRequest {
            requirement: requirement(),
            providers: vec![provider(
                "provider:from-env",
                SecretProviderTier::ExplicitEnvironmentImport,
                [SecretMaterialisationClass::ProcessEnv],
            )],
            headless: true,
            allow_from_env: false,
        })
        .unwrap();

        assert!(!result.selected());
        assert_eq!(
            result.provider_explanations[0].rejection,
            Some(CredentialProviderRejection::EnvironmentImportNotExplicitlyAllowed)
        );
    }

    #[test]
    fn explicit_headless_from_env_is_visible_as_degraded_provider_choice() {
        let mut env = provider(
            "provider:from-env",
            SecretProviderTier::ExplicitEnvironmentImport,
            [SecretMaterialisationClass::ProcessEnv],
        );
        env.degradation = Some("operator-supplied headless environment import".into());

        let result = resolve_credential(CredentialResolutionRequest {
            requirement: requirement(),
            providers: vec![env],
            headless: true,
            allow_from_env: true,
        })
        .unwrap();

        assert!(result.selected());
        assert_eq!(
            result.degradation.as_deref(),
            Some("operator-supplied headless environment import")
        );
    }

    #[test]
    fn read_model_contains_no_raw_secret_material() {
        let raw_secret = "sk-fixture-DO-NOT-SERIALIZE";
        let result = resolve_credential(CredentialResolutionRequest {
            requirement: requirement(),
            providers: vec![provider(
                "provider:varlock-1password",
                SecretProviderTier::BrokeredSecureProvider,
                [SecretMaterialisationClass::CredentialBroker],
            )],
            headless: true,
            allow_from_env: false,
        })
        .unwrap();

        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains(raw_secret));
        assert!(json.contains("credential:openai/research"));
        assert!(json.contains("provider:varlock-1password"));
    }

    #[test]
    fn rotation_changes_provider_revision_not_credential_ref() {
        let base = CredentialBindingState {
            credential_ref: credential("credential:openai/research"),
            provider_ref: SecretProviderRef::new("provider:keychain").unwrap(),
            binding_provenance: "keychain:item-42".into(),
            revision_or_lease_class: Some("revision:v1".into()),
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
        };
        let rotated = CredentialBindingState {
            revision_or_lease_class: Some("revision:v2".into()),
            ..base.clone()
        };

        assert_eq!(base.credential_ref, rotated.credential_ref);
        assert_ne!(base.revision_or_lease_class, rotated.revision_or_lease_class);
    }

    #[test]
    fn provider_replacement_changes_provenance_not_credential_ref() {
        let first = CredentialBindingState {
            credential_ref: credential("credential:openai/research"),
            provider_ref: SecretProviderRef::new("provider:keychain").unwrap(),
            binding_provenance: "keychain:item-42".into(),
            revision_or_lease_class: None,
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
        };
        let replacement = CredentialBindingState {
            credential_ref: first.credential_ref.clone(),
            provider_ref: SecretProviderRef::new("provider:varlock-1password").unwrap(),
            binding_provenance: "op://Agent/provider-key".into(),
            revision_or_lease_class: None,
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
        };

        assert_eq!(first.credential_ref, replacement.credential_ref);
        assert_ne!(first.provider_ref, replacement.provider_ref);
        assert_ne!(first.binding_provenance, replacement.binding_provenance);
    }
}
