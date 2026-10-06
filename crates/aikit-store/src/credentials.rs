//! Provider-neutral credential binding records.
//!
//! This store persists only [`CredentialBindingState`]. The type has no secret
//! field by construction, so raw authentication material cannot enter this
//! persistence path.

use std::fs;
use std::io::Write;
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

    /// Legacy unconditional save remains serialized and atomically durable.
    /// Mutation owners with a read basis must use compare_and_save instead.
    pub fn save(&self, state: &CredentialBindingState) -> Result<()> {
        self.write_locked(state, None)
    }

    /// Stable public metadata basis, never a digest of credential material.
    pub fn revision(state: Option<&CredentialBindingState>) -> Result<String> {
        match state {
            None => Ok("absent".into()),
            Some(state) => {
                let bytes = serde_json::to_vec(state).map_err(|_| {
                    AikitError::new(
                        "credential.binding_encode_failed",
                        "could not encode binding basis",
                    )
                })?;
                Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
            }
        }
    }

    /// Compare inside the one per-credential interprocess owner lock. A late
    /// verify/rotate/revoke can never overwrite a different persisted basis.
    pub fn compare_and_save(
        &self,
        expected: Option<&CredentialBindingState>,
        state: &CredentialBindingState,
    ) -> Result<()> {
        self.write_locked(state, Some(expected))
    }

    fn write_locked(
        &self,
        state: &CredentialBindingState,
        expected: Option<Option<&CredentialBindingState>>,
    ) -> Result<()> {
        self.home.ensure_layout()?;
        let path = self.path(&state.credential_ref);
        let lock_path = path.with_extension("lock");
        let _lock = crate::locks::ContextLock::acquire_at(
            &lock_path,
            &format!("credential-binding:{}", state.credential_ref.as_str()),
            crate::locks::LockOptions::default()
                .with_purpose("credential binding compare-and-save"),
        )?;
        restrict_file(&lock_path)?;
        // File lock is released by drop on every success/error path.
        if let Some(expected) = expected {
            let actual = self.load(&state.credential_ref)?;
            if actual.as_ref() != expected {
                return Err(AikitError::new(
                    "credential.binding_stale",
                    "credential binding changed; reread its actual basis before retrying",
                ));
            }
        }
        let temporary = path.with_extension(format!("{}.tmp", ulid::Ulid::generate()));
        let result = (|| {
            let bytes = serde_json::to_vec_pretty(state).map_err(|error| {
                AikitError::new(
                    "credential.binding_encode_failed",
                    format!("could not encode credential binding: {error}"),
                )
            })?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|error| {
                AikitError::new(
                    "credential.binding_write_failed",
                    format!("could not create unique binding temporary: {error}"),
                )
            })?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|error| {
                    AikitError::new(
                        "credential.binding_write_failed",
                        format!("could not durably write binding: {error}"),
                    )
                })?;
            restrict_file(&temporary)?;
            fs::rename(&temporary, &path).map_err(|error| {
                AikitError::new(
                    "credential.binding_write_failed",
                    format!("could not replace binding: {error}"),
                )
            })?;
            #[cfg(unix)]
            fs::File::open(self.home.credentials())
                .and_then(|dir| dir.sync_all())
                .map_err(|error| {
                    AikitError::new(
                        "credential.binding_write_failed",
                        format!("could not sync credential directory: {error}"),
                    )
                })?;
            Ok(())
        })();
        if result.is_err() {
            // Preserve the first failure. A unique, incomplete file cannot be
            // mistaken for an authoritative .json record by list/read/restart.
            let _ = fs::remove_file(&temporary);
        }
        result
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
                        format!(
                            "invalid credential binding {}: {error}",
                            entry.path().display()
                        ),
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
            harness_auth_source: None,
            declared_secret_ref: Some(
                aikit_core::secret_ref::SecretRef::parse("op://Vault/openai/key").unwrap(),
            ),
            bound_at_unix_seconds: Some(1_700_000_000),
            last_rotated_at_unix_seconds: None,
            last_verified_at_unix_seconds: None,
        };

        store.save(&state).unwrap();
        assert_eq!(
            store.load(&state.credential_ref).unwrap(),
            Some(state.clone())
        );
        assert_eq!(store.list().unwrap(), vec![state]);

        let raw = fs::read_dir(home.credentials())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .find(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .unwrap()
            .path();
        let text = fs::read_to_string(raw).unwrap();
        assert!(text.contains("os-secure-store"));
        assert!(text.contains("provider-native-lease"));
        assert!(!text.contains("sk-"));
    }
    fn fixture_state() -> CredentialBindingState {
        serde_json::from_value(serde_json::json!({
            "credential_ref": "credential:test/cas",
            "provider_ref": "provider:test/source",
            "provider_tier": "brokered-secure-provider",
            "materialisation": "process-env",
            "binding_provenance": "synthetic isolated store",
            "revision_or_lease_class": "test/v1", "expires_at": null,
            "revoked": false, "metadata": {}, "bound_at_unix_seconds": 1
        }))
        .unwrap()
    }

    #[test]
    fn concurrent_replies_admit_one_exact_basis_and_restart_retains_winner() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let store = CredentialBindingStore::new(&home);
        let initial = fixture_state();
        store.compare_and_save(None, &initial).unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut replies = Vec::new();
        for revoked in [true, false] {
            let barrier = barrier.clone();
            let home = home.clone();
            let initial = initial.clone();
            replies.push(std::thread::spawn(move || {
                let store = CredentialBindingStore::new(&home);
                let mut candidate = initial.clone();
                if revoked {
                    candidate.revoked = true;
                } else {
                    candidate.last_verified_at_unix_seconds = Some(2);
                }
                barrier.wait();
                (
                    store.compare_and_save(Some(&initial), &candidate).is_ok(),
                    candidate,
                )
            }));
        }
        barrier.wait();
        let results: Vec<_> = replies.into_iter().map(|t| t.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|(ok, _)| *ok).count(), 1);
        let winner = &results.iter().find(|(ok, _)| *ok).unwrap().1;
        let restarted = CredentialBindingStore::new(&AikitHome::at(temp.path()));
        assert_eq!(
            restarted.load(&initial.credential_ref).unwrap().as_ref(),
            Some(winner)
        );
        assert_eq!(restarted.list().unwrap(), vec![winner.clone()]);
        assert!(fs::read_dir(home.credentials()).unwrap().all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_none_or(|e| e != "tmp")));
    }

    #[test]
    fn stale_verify_cannot_undo_revocation_and_orphan_temporary_cannot_certify() {
        let temp = tempfile::tempdir().unwrap();
        let home = AikitHome::at(temp.path());
        let store = CredentialBindingStore::new(&home);
        let initial = fixture_state();
        store.compare_and_save(None, &initial).unwrap();
        let mut revoked = initial.clone();
        revoked.revoked = true;
        store.compare_and_save(Some(&initial), &revoked).unwrap();
        let mut late = initial.clone();
        late.last_verified_at_unix_seconds = Some(3);
        assert_eq!(
            store
                .compare_and_save(Some(&initial), &late)
                .unwrap_err()
                .code(),
            "credential.binding_stale"
        );
        fs::write(
            store
                .path(&initial.credential_ref)
                .with_extension("orphan.tmp"),
            b"{partial",
        )
        .unwrap();
        let restarted = CredentialBindingStore::new(&home);
        assert_eq!(
            restarted.load(&initial.credential_ref).unwrap(),
            Some(revoked.clone())
        );
        assert_eq!(restarted.list().unwrap(), vec![revoked]);
    }

    #[test]
    fn credential_cas_process_child() {
        let Some(root) = std::env::var_os("AIKIT_TEST_CREDENTIAL_CAS_ROOT") else {
            return;
        };
        let side = std::env::var("AIKIT_TEST_CREDENTIAL_CAS_SIDE").unwrap();
        let root = std::path::PathBuf::from(root);
        let store = CredentialBindingStore::new(&AikitHome::at(root.join("state")));
        let expected: CredentialBindingState =
            serde_json::from_slice(&fs::read(root.join("expected.json")).unwrap()).unwrap();
        let mut candidate = expected.clone();
        if side == "revoke" {
            candidate.revoked = true;
        } else {
            candidate.last_verified_at_unix_seconds = Some(9);
        }
        fs::write(root.join(format!("{side}.ready")), b"ready").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !root.join("go").exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "bounded process owner gate timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let result = match store.compare_and_save(Some(&expected), &candidate) {
            Ok(()) => "admitted",
            Err(error) if error.code() == "credential.binding_stale" => "stale",
            Err(error) => panic!("actual owner failed: {error}"),
        };
        fs::write(root.join(format!("{side}.result")), result).unwrap();
    }

    #[test]
    fn separate_native_processes_cannot_overwrite_each_others_expected_basis() {
        let root = tempfile::tempdir().unwrap();
        let store = CredentialBindingStore::new(&AikitHome::at(root.path().join("state")));
        let expected = fixture_state();
        store.compare_and_save(None, &expected).unwrap();
        fs::write(
            root.path().join("expected.json"),
            serde_json::to_vec(&expected).unwrap(),
        )
        .unwrap();
        let mut children = Vec::new();
        for side in ["revoke", "verify"] {
            children.push(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "credentials::tests::credential_cas_process_child",
                        "--nocapture",
                    ])
                    .env("AIKIT_TEST_CREDENTIAL_CAS_ROOT", root.path())
                    .env("AIKIT_TEST_CREDENTIAL_CAS_SIDE", side)
                    .spawn()
                    .unwrap(),
            );
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !root.path().join("revoke.ready").exists()
            || !root.path().join("verify.ready").exists()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "bounded child startup timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        fs::write(root.path().join("go"), b"go").unwrap();
        for child in &mut children {
            assert!(child.wait().unwrap().success());
        }
        let result: Vec<_> = ["revoke", "verify"]
            .iter()
            .map(|side| fs::read_to_string(root.path().join(format!("{side}.result"))).unwrap())
            .collect();
        assert_eq!(
            result.iter().filter(|s| s.as_str() == "admitted").count(),
            1
        );
        assert_eq!(result.iter().filter(|s| s.as_str() == "stale").count(), 1);
        let retained = store.load(&expected.credential_ref).unwrap().unwrap();
        assert_eq!(retained.revoked, result[0] == "admitted");
        assert_eq!(
            retained.last_verified_at_unix_seconds,
            if retained.revoked { None } else { Some(9) }
        );
    }
}
