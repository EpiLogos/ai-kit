//! The one credential-delivery seam for harness launches.
//!
//! Two paths deliver a bound provider key into a scrubbed final-child
//! environment: the selected-model policy names its credential and target
//! variable explicitly (the pi dispatch path), and the harness profile /
//! route launcher delivers under a profile-declared variable. Both call the
//! single [`credential`] here, so the materialisation rules — declared ref
//! through the resolver suite, native secure store, explicit env import,
//! never an ambient value, never an empty pass-through — exist exactly once.
//!
//! Raw material is not serializable and is never stored in any receipt,
//! journal or read model; it exists only in the spawned `Command`.

use aikit_adapters::credential_provider::{EnvironmentImportProvider, NativeSecureStoreProvider};
use aikit_core::credential::{
    resolve_credential, valid_credential_variable, CredentialRef, CredentialResolutionRequest,
    SecretMaterialisationClass, SecretProvider, SecretRequirement, SecretRequirementRef,
    SecretValue,
};
use aikit_core::secret_ref::SecretResolver as _;
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{AikitHome, CredentialBindingStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("credential.delivery", message.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelCredential {
    pub requirement_ref: SecretRequirementRef,
    pub credential_ref: CredentialRef,
    pub target_env: String,
    /// Explicit import, never inferred from an ambient variable or a .env file.
    pub from_env: Option<String>,
}

/// The production declared-ref resolution: the resolver suite, env gate
/// closed. Injectable at [`credential_resolved`] so tests script the store
/// boundary instead of touching a live vault — the same convention the
/// resolver suite's own `OpRead`/`VarlockRead` traits set.
pub(crate) fn default_declared_resolver(secret_ref: &aikit_core::SecretRef) -> Result<SecretValue> {
    aikit_adapters::secret_resolver::SuiteSecretResolver::default().resolve(secret_ref)
}

/// Resolve one credential binding and, when `materialise` is set, return its
/// secret. A declared reference materialises straight from the external store
/// the operator named; when only planning, the vault is never touched and the
/// returned secret is `None`. A revoked or expired binding refuses either way.
pub(crate) fn credential(
    home: &AikitHome,
    session: &ResourceRef,
    use_: &ModelCredential,
    materialise: bool,
) -> Result<(Value, Option<SecretValue>)> {
    credential_resolved(home, session, use_, materialise, &default_declared_resolver)
}

/// [`credential`] with the declared-ref resolution injected.
pub(crate) fn credential_resolved(
    home: &AikitHome,
    session: &ResourceRef,
    use_: &ModelCredential,
    materialise: bool,
    resolve_declared: &dyn Fn(&aikit_core::SecretRef) -> Result<SecretValue>,
) -> Result<(Value, Option<SecretValue>)> {
    if !valid_credential_variable(&use_.target_env)
        || use_
            .from_env
            .as_ref()
            .is_some_and(|v| !valid_credential_variable(v))
    {
        return Err(error("Model credentials need explicit non-control credential variable names; environment control injection is refused"));
    }
    let stored = CredentialBindingStore::new(home).load(&use_.credential_ref)?;
    if let Some(binding) = &stored {
        if binding.revoked
            || binding
                .expires_at
                .as_deref()
                .map(|s| s.parse::<jiff::Timestamp>().map_err(error))
                .transpose()?
                .is_some_and(|t| t <= jiff::Timestamp::now())
        {
            return Err(error(
                "Selected credential binding is revoked or expired; no environment bypass",
            ));
        }
    }
    // A declared reference materialises through the resolver suite straight
    // from the external store the operator named (1Password, varlock, pass,
    // keychain). The suite's env-import gate is closed by construction, so
    // env:// can never ride this path; the ref itself was refused at the
    // setup seam. When only planning (materialise == false) the vault is
    // never touched: the delivery record names the route without resolving.
    if let Some(secret_ref) = stored
        .as_ref()
        .and_then(|binding| binding.declared_secret_ref.clone())
    {
        let secret = if materialise {
            Some(resolve_declared(&secret_ref)?)
        } else {
            None
        };
        return Ok((
            json!({
                "binding": stored,
                "delivery": "declared-secret-ref",
                "scheme": secret_ref.scheme(),
                "secret_persisted": false,
                "resolution": Value::Null,
            }),
            secret,
        ));
    }
    // Native secure-store eligibility is the conjunction of a usable platform
    // backend and the current provider-neutral binding record. Constructing an
    // empty provider here discarded the latter, so the launch path reported a
    // credential as absent even while `credential explain` selected it.
    let native = NativeSecureStoreProvider::with_binding(stored.as_ref());
    let environment = use_
        .from_env
        .as_ref()
        .map(|name| {
            EnvironmentImportProvider::from_process(use_.credential_ref.clone(), name, None)
        })
        .transpose()?;
    let mut descriptors = vec![native.descriptor(&use_.credential_ref)];
    if let Some(env) = &environment {
        descriptors.push(env.descriptor(&use_.credential_ref));
    }
    let resolution = resolve_credential(CredentialResolutionRequest {
        requirement: SecretRequirement {
            requirement_ref: use_.requirement_ref.clone(),
            credential_ref: use_.credential_ref.clone(),
            consumer_ref: session.to_string(),
            purpose: "Scoped model dispatch into the selected native resident".into(),
            permitted_materialisation: [SecretMaterialisationClass::ProcessEnv].into(),
        },
        providers: descriptors,
        headless: true,
        allow_from_env: use_.from_env.is_some(),
    })?;
    let provider = resolution.selected_provider_ref.as_ref().ok_or_else(|| {
        error("No eligible current credential provider; an inventory reference is not key material")
    })?;
    let secret = if materialise {
        let source: &dyn SecretProvider =
            if native.descriptor(&use_.credential_ref).provider_ref == *provider {
                &native
            } else {
                environment
                .as_ref()
                .filter(|e| e.descriptor(&use_.credential_ref).provider_ref == *provider)
                .ok_or_else(|| {
                    error("Selected credential provider is not materialisable by this native path")
                })?
            };
        Some(source.materialise(&use_.credential_ref, SecretMaterialisationClass::ProcessEnv)?
            .ok_or_else(|| error("Selected credential provider did not return material; refusing provider execution"))?)
    } else {
        None
    };
    Ok((
        json!({"resolution":resolution,"binding":stored,"delivery":"process-env", "secret_persisted":false}),
        secret,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::credential::{CredentialBindingState, SecretProviderRef, SecretProviderTier};
    use std::collections::BTreeMap;

    fn use_() -> ModelCredential {
        ModelCredential {
            requirement_ref: SecretRequirementRef::new(
                "secret-requirement:openrouter-native-dispatch",
            )
            .unwrap(),
            credential_ref: CredentialRef::new("openrouter").unwrap(),
            target_env: "OPENROUTER_API_KEY".into(),
            from_env: None,
        }
    }

    fn binding(use_: &ModelCredential, provider_ref: SecretProviderRef) -> CredentialBindingState {
        CredentialBindingState {
            credential_ref: use_.credential_ref.clone(),
            provider_ref,
            provider_tier: SecretProviderTier::OsSecureStore,
            materialisation: SecretMaterialisationClass::ProviderNativeLease,
            binding_provenance: "native-store-test-binding".into(),
            revision_or_lease_class: Some("keyring-test".into()),
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
            declared_secret_ref: None,
            bound_at_unix_seconds: Some(1_700_000_000),
            last_rotated_at_unix_seconds: None,
            last_verified_at_unix_seconds: Some(1_700_000_001),
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn current_native_binding_is_eligible_without_materialising_keychain_data() {
        let temporary = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temporary.path());
        let use_ = use_();
        let empty = NativeSecureStoreProvider::new();
        let descriptor = empty.descriptor(&use_.credential_ref);
        assert!(
            descriptor.available,
            "macOS Keychain entry construction is required"
        );
        CredentialBindingStore::new(&home)
            .save(&binding(&use_, descriptor.provider_ref.clone()))
            .unwrap();

        let (reading, secret) = credential(
            &home,
            &ResourceRef::parse("agent-session/native-binding-test").unwrap(),
            &use_,
            false,
        )
        .unwrap();

        assert!(
            secret.is_none(),
            "eligibility must not read credential material"
        );
        assert_eq!(
            reading["resolution"]["selected_provider_ref"],
            json!(descriptor.provider_ref)
        );
        assert_eq!(reading["binding"]["credential_ref"], "openrouter");
        assert_eq!(reading["secret_persisted"], false);
        assert!(!reading.to_string().contains("OPENROUTER_API_KEY"));
    }

    #[test]
    fn absent_and_revoked_native_bindings_remain_ineligible() {
        let temporary = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temporary.path());
        let use_ = use_();
        let session = ResourceRef::parse("agent-session/native-binding-negative").unwrap();

        assert_eq!(
            credential(&home, &session, &use_, false)
                .unwrap_err()
                .message(),
            "No eligible current credential provider; an inventory reference is not key material"
        );

        let descriptor = NativeSecureStoreProvider::new().descriptor(&use_.credential_ref);
        let mut revoked = binding(&use_, descriptor.provider_ref);
        revoked.revoked = true;
        CredentialBindingStore::new(&home).save(&revoked).unwrap();
        assert_eq!(
            credential(&home, &session, &use_, false)
                .unwrap_err()
                .message(),
            "Selected credential binding is revoked or expired; no environment bypass"
        );
    }
}
