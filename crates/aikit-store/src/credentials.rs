//! Provider-neutral credential binding records.
//!
//! This store persists only [`CredentialBindingState`]. The type has no secret
//! field by construction, so raw authentication material cannot enter this
//! persistence path.

use std::fs;
use std::path::PathBuf;

use aikit_core::credential::{CredentialBindingState, CredentialRef};
use aikit_core::{AikitError, Result};

use crate::home::AikitHome;

pub const CREDENTIAL_BINDING_STORE_VERSION: &str = "aikit.credential-bindings/v1";

#[derive(Debug, Clone)]
pub struct CredentialBindingStore {
    home: AikitHome,
}

impl CredentialBindingStore {
    pub fn new(home: &AikitHome) -> Self {
        Self { home: home.clone() }
    }

    fn path(&self, credential_ref: &CredentialRef) -> PathBuf {
        let digest = blake3::hash(credential_ref.as_str().as_bytes());
        self.home
            .credentials()
            .join(format!("{}.json", digest.to_hex()))
    }

    pub fn save(&self, state: &CredentialBindingState) -> Result<()> {
        self.home.ensure_layout()?;
        let path = self.path(&state.credential_ref);
        let temporary = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
            AikitError::new(
                "credential.binding_encode_failed",
                format!("could not encode credential binding state: {error}"),
            )
        })?;
        fs::write(&temporary, bytes).map_err(|error| {
            AikitError::new(
                "credential.binding_write_failed",
                format!("could not write {}: {error}", temporary.display()),
            )
            .with("path", temporary.display().to_string())
        })?;
        restrict_file(&temporary)?;
        fs::rename(&temporary, &path).map_err(|error| {
            AikitError::new(
                "credential.binding_write_failed",
                format!("could not replace {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        Ok(())
    }

    pub fn load(&self, credential_ref: &CredentialRef) -> Result<Option<CredentialBindingState>> {
        let path = self.path(credential_ref);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AikitError::new(
                    "credential.binding_read_failed",
                    format!("could not read {}: {error}", path.display()),
                )
                .with("path", path.display().to_string()))
            }
        };
        let state: CredentialBindingState = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "credential.binding_invalid",
                format!("invalid credential binding {}: {error}", path.display()),
            )
            .with("path", path.display().to_string())
        })?;
        if state.credential_ref != *credential_ref {
            return Err(AikitError::new(
                "credential.binding_invalid",
                "credential binding identity does not match its storage key",
            )
            .with("path", path.display().to_string()));
        }
        Ok(Some(state))
    }

    pub fn list(&self) -> Result<Vec<CredentialBindingState>> {
        let directory = self.home.credentials();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => {
                return Err(AikitError::new(
                    "credential.binding_read_failed",
                    format!("could not read {}: {error}", directory.display()),
                ))
            }
        };
        let mut bindings = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                AikitError::new(
                    "credential.binding_read_failed",
                    format!("could not enumerate credential bindings: {error}"),
                )
            })?;
            if entry.path().extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(entry.path()).map_err(|error| {
                AikitError::new(
                    "credential.binding_read_failed",
                    format!("could not read {}: {error}", entry.path().display()),
                )
            })?;
            let state: CredentialBindingState =
                serde_json::from_slice(&bytes).map_err(|error| {
                    AikitError::new(
                        "credential.binding_invalid",
                        format!("invalid credential binding {}: {error}", entry.path().display()),
                    )
                })?;
            bindings.push(state);
        }
        bindings.sort_by(|a, b| a.credential_ref.cmp(&b.credential_ref));
        Ok(bindings)
    }
}

#[cfg(unix)]
fn restrict_file(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        AikitError::new(
            "credential.binding_write_failed",
            format!("could not restrict {}: {error}", path.display()),
        )
    })
}

#[cfg(not(unix))]
fn restrict_file(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::credential::{
        SecretMaterialisationClass, SecretProviderRef, SecretProviderTier,
    };
    use std::collections::BTreeMap;

    #[test]
    fn binding_store_round_trips_only_safe_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let store = CredentialBindingStore::new(&home);
        let state = CredentialBindingState {
            credential_ref: CredentialRef::new("credential:test/store").unwrap(),
            provider_ref: SecretProviderRef::new("provider:test/keychain").unwrap(),
            provider_tier: SecretProviderTier::OsSecureStore,
            materialisation: SecretMaterialisationClass::ProviderNativeLease,
            binding_provenance: "keychain:test-item".into(),
            revision_or_lease_class: Some("keyring-v1".into()),
            expires_at: None,
            revoked: false,
            metadata: BTreeMap::new(),
        };

        store.save(&state).unwrap();
        assert_eq!(store.load(&state.credential_ref).unwrap(), Some(state.clone()));
        assert_eq!(store.list().unwrap(), vec![state]);

        let raw = fs::read_dir(home.credentials())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let text = fs::read_to_string(raw).unwrap();
        assert!(text.contains("os-secure-store"));
        assert!(text.contains("provider-native-lease"));
        assert!(!text.contains("sk-"));
    }
}