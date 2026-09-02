//! Native and explicit-import implementations of the provider-neutral credential seam.
//!
//! Authentication material remains provider-owned. These adapters expose only
//! descriptors and safe binding metadata to the read model; raw secret values
//! exist solely inside [`SecretValue`] long enough to bind or materialise them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

use aikit_core::credential::{
    CredentialBindingState, CredentialRef, SecretMaterialisationClass, SecretProvider,
    SecretProviderDescriptor, SecretProviderRef, SecretProviderTier, SecretValue,
};
use aikit_core::{AikitError, Result};
use keyring::v1::{Entry, Error as KeyringError};

const AIKIT_KEYRING_SERVICE: &str = "dev.aikit.credentials";
const KEYRING_REVISION: &str = "keyring-v1/4.1.5";

fn provider_error(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

fn native_provider_ref() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "provider:os-secure-store/macos-keychain";
    }
    #[cfg(target_os = "linux")]
    {
        return "provider:os-secure-store/linux-secret-service";
    }
    #[cfg(target_os = "windows")]
    {
        return "provider:os-secure-store/windows-credential-manager";
    }
    #[allow(unreachable_code)]
    "provider:os-secure-store/unsupported"
}

fn native_provider_kind() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "macos-keychain";
    }
    #[cfg(target_os = "linux")]
    {
        return "linux-secret-service";
    }
    #[cfg(target_os = "windows")]
    {
        return "windows-credential-manager";
    }
    #[allow(unreachable_code)]
    "unsupported-os-secure-store"
}

fn native_assurance() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "OS-native Keychain Services item; secret material is retained by macOS";
    }
    #[cfg(target_os = "linux")]
    {
        return "Secret Service item through org.freedesktop.secrets; secret material is retained by the desktop keyring";
    }
    #[cfg(target_os = "windows")]
    {
        return "OS-native Windows Credential Manager entry; secret material is retained by Windows";
    }
    #[allow(unreachable_code)]
    "native secure storage is not supported on this platform"
}

fn binding_provenance(credential_ref: &CredentialRef) -> String {
    format!(
        "{}:service={AIKIT_KEYRING_SERVICE};account={}",
        native_provider_kind(),
        credential_ref.as_str()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeSecureStoreStatus {
    Bound,
    Unbound,
    Unavailable,
}

/// Cross-platform OS secure-store provider.
///
/// The `keyring` v1 adapter is intentionally narrow: on macOS it selects
/// Keychain Services, on Linux Secret Service, and on Windows Credential
/// Manager. AIKit never serializes the retrieved secret.
pub struct NativeSecureStoreProvider {
    bound_credentials: BTreeSet<CredentialRef>,
}

impl NativeSecureStoreProvider {
    pub fn new() -> Self {
        Self {
            bound_credentials: BTreeSet::new(),
        }
    }

    /// Construct from safe binding metadata. Checking whether a credential is
    /// bound must never materialise the secret from the native store.
    pub fn with_binding(binding: Option<&CredentialBindingState>) -> Self {
        let mut provider = Self::new();
        if let Some(binding) = binding.filter(|binding| {
            binding.provider_tier == SecretProviderTier::OsSecureStore
                && binding.provider_ref.as_str() == native_provider_ref()
                && !binding.revoked
        }) {
            provider
                .bound_credentials
                .insert(binding.credential_ref.clone());
        }
        provider
    }

    fn entry(credential_ref: &CredentialRef) -> std::result::Result<Entry, KeyringError> {
        Entry::new(AIKIT_KEYRING_SERVICE, credential_ref.as_str())
    }

    pub fn status(&self, credential_ref: &CredentialRef) -> NativeSecureStoreStatus {
        // Initialising an Entry determines whether the platform backend exists,
        // but deliberately does not retrieve credential material. Binding
        // presence comes from AIKit's provider-neutral metadata record.
        if Self::entry(credential_ref).is_err() {
            return NativeSecureStoreStatus::Unavailable;
        }
        if self.bound_credentials.contains(credential_ref) {
            NativeSecureStoreStatus::Bound
        } else {
            NativeSecureStoreStatus::Unbound
        }
    }

    pub fn delete(&self, credential_ref: &CredentialRef) -> Result<()> {
        let entry = Self::entry(credential_ref).map_err(|error| {
            provider_error(
                "credential.native_store_unavailable",
                format!("could not open native credential store: {error}"),
            )
        })?;
        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(provider_error(
                "credential.native_store_delete_failed",
                format!("could not delete native credential binding: {error}"),
            )),
        }
    }

    fn state(&self, credential_ref: &CredentialRef) -> CredentialBindingState {
        let mut metadata = BTreeMap::new();
        metadata.insert("provider_kind".into(), native_provider_kind().into());
        metadata.insert("service".into(), AIKIT_KEYRING_SERVICE.into());
        CredentialBindingState {
            credential_ref: credential_ref.clone(),
            provider_ref: SecretProviderRef::new(native_provider_ref())
                .expect("native provider ref is static and valid"),
            provider_tier: SecretProviderTier::OsSecureStore,
            materialisation: SecretMaterialisationClass::ProviderNativeLease,
            binding_provenance: binding_provenance(credential_ref),
            revision_or_lease_class: Some(KEYRING_REVISION.into()),
            expires_at: None,
            revoked: false,
            metadata,
        }
    }
}

impl Default for NativeSecureStoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretProvider for NativeSecureStoreProvider {
    fn descriptor(&self, credential_ref: &CredentialRef) -> SecretProviderDescriptor {
        let status = self.status(credential_ref);
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new(native_provider_ref())
                .expect("native provider ref is static and valid"),
            provider_kind: native_provider_kind().into(),
            tier: SecretProviderTier::OsSecureStore,
            available: status != NativeSecureStoreStatus::Unavailable,
            // Headless means "never ask AIKit's prompt". Native platform policy may
            // still reject access, but that is returned deterministically as unavailable.
            headless_capable: true,
            assurance: native_assurance().into(),
            degradation: None,
            supported_credentials: if status == NativeSecureStoreStatus::Bound {
                [credential_ref.clone()].into_iter().collect()
            } else {
                BTreeSet::new()
            },
            supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                .into_iter()
                .collect(),
            binding_provenance: binding_provenance(credential_ref),
            revision_or_lease_class: Some(KEYRING_REVISION.into()),
        }
    }

    fn binding_state(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<Option<CredentialBindingState>> {
        Ok(
            (self.status(credential_ref) == NativeSecureStoreStatus::Bound)
                .then(|| self.state(credential_ref)),
        )
    }

    fn bind(
        &self,
        credential_ref: &CredentialRef,
        secret: &SecretValue,
    ) -> Result<CredentialBindingState> {
        let entry = Self::entry(credential_ref).map_err(|error| {
            provider_error(
                "credential.native_store_unavailable",
                format!("could not open native credential store: {error}"),
            )
        })?;
        entry.set_password(secret.expose()).map_err(|error| {
            provider_error(
                "credential.native_store_write_failed",
                format!("could not bind credential in native secure store: {error}"),
            )
        })?;
        Ok(self.state(credential_ref))
    }

    fn materialise(
        &self,
        credential_ref: &CredentialRef,
        class: SecretMaterialisationClass,
    ) -> Result<Option<SecretValue>> {
        if class != SecretMaterialisationClass::ProviderNativeLease {
            return Ok(None);
        }
        let entry = Self::entry(credential_ref).map_err(|error| {
            provider_error(
                "credential.native_store_unavailable",
                format!("could not open native credential store: {error}"),
            )
        })?;
        match entry.get_password() {
            Ok(secret) => SecretValue::new(secret).map(Some),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(provider_error(
                "credential.native_store_read_failed",
                format!("could not materialise native credential: {error}"),
            )),
        }
    }
}

/// Explicit environment import. Merely constructing or discovering this provider
/// never makes it eligible: the core resolver still requires `allow_from_env`.
pub struct EnvironmentImportProvider {
    credential_ref: CredentialRef,
    env_var: String,
    source_available: bool,
    value: Option<SecretValue>,
    provenance: String,
}

impl EnvironmentImportProvider {
    /// Discover a declared environment route without reading its secret value.
    /// Existence is intentionally not probed here: the route is discoverable,
    /// while actual source presence is tested only after explicit `--from-env`.
    pub fn discover(
        credential_ref: CredentialRef,
        env_var: impl Into<String>,
        project_env: Option<&Path>,
    ) -> Result<Self> {
        let env_var = env_var.into();
        if env_var.trim().is_empty() {
            return Err(provider_error(
                "credential.env_var_invalid",
                "environment variable name must not be empty",
            ));
        }
        let provenance = project_env
            .map(|path| format!("project-env:{}#{env_var}", path.display()))
            .unwrap_or_else(|| format!("shell-env:{env_var}"));
        Ok(Self {
            credential_ref,
            env_var,
            source_available: true,
            value: None,
            provenance,
        })
    }

    pub fn from_process(
        credential_ref: CredentialRef,
        env_var: impl Into<String>,
        project_env: Option<&Path>,
    ) -> Result<Self> {
        let env_var = env_var.into();
        if env_var.trim().is_empty() {
            return Err(provider_error(
                "credential.env_var_invalid",
                "environment variable name must not be empty",
            ));
        }

        if let Ok(value) = std::env::var(&env_var) {
            if !value.is_empty() {
                return Ok(Self {
                    credential_ref,
                    provenance: format!("shell-env:{env_var}"),
                    env_var,
                    source_available: true,
                    value: Some(SecretValue::new(value)?),
                });
            }
        }

        if let Some(path) = project_env {
            if let Some(value) = read_dotenv_value(path, &env_var)? {
                return Ok(Self {
                    credential_ref,
                    provenance: format!("project-env:{}#{env_var}", path.display()),
                    env_var,
                    source_available: true,
                    value: Some(SecretValue::new(value)?),
                });
            }
        }

        Ok(Self {
            credential_ref,
            provenance: format!("shell-env:{env_var}"),
            env_var,
            source_available: false,
            value: None,
        })
    }

    /// Test/application seam that avoids mutating the process environment.
    pub fn from_value(
        credential_ref: CredentialRef,
        env_var: impl Into<String>,
        value: Option<String>,
    ) -> Result<Self> {
        let env_var = env_var.into();
        let source_available = value.is_some();
        let value = value.map(SecretValue::new).transpose()?;
        Ok(Self {
            credential_ref,
            provenance: format!("shell-env:{env_var}"),
            env_var,
            source_available,
            value,
        })
    }

    fn state(&self) -> CredentialBindingState {
        let mut metadata = BTreeMap::new();
        metadata.insert("env_var".into(), self.env_var.clone());
        CredentialBindingState {
            credential_ref: self.credential_ref.clone(),
            provider_ref: SecretProviderRef::new("provider:explicit-environment-import")
                .expect("static provider ref is valid"),
            provider_tier: SecretProviderTier::ExplicitEnvironmentImport,
            materialisation: SecretMaterialisationClass::ProcessEnv,
            binding_provenance: self.provenance.clone(),
            revision_or_lease_class: Some("process-environment/snapshot".into()),
            expires_at: None,
            revoked: false,
            metadata,
        }
    }
}

impl SecretProvider for EnvironmentImportProvider {
    fn descriptor(&self, credential_ref: &CredentialRef) -> SecretProviderDescriptor {
        let bound = credential_ref == &self.credential_ref && self.source_available;
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new("provider:explicit-environment-import")
                .expect("static provider ref is valid"),
            provider_kind: "explicit-environment-import".into(),
            tier: SecretProviderTier::ExplicitEnvironmentImport,
            available: true,
            headless_capable: true,
            assurance: "operator-authorised import from a named shell/project environment source"
                .into(),
            degradation: Some(
                "environment import is the lowest-assurance credential tier and is never promoted"
                    .into(),
            ),
            supported_credentials: if bound {
                [credential_ref.clone()].into_iter().collect()
            } else {
                BTreeSet::new()
            },
            supported_materialisation: [SecretMaterialisationClass::ProcessEnv]
                .into_iter()
                .collect(),
            binding_provenance: self.provenance.clone(),
            revision_or_lease_class: Some("process-environment/snapshot".into()),
        }
    }

    fn binding_state(
        &self,
        credential_ref: &CredentialRef,
    ) -> Result<Option<CredentialBindingState>> {
        Ok((credential_ref == &self.credential_ref && self.value.is_some()).then(|| self.state()))
    }

    fn bind(
        &self,
        _credential_ref: &CredentialRef,
        _secret: &SecretValue,
    ) -> Result<CredentialBindingState> {
        Err(provider_error(
            "credential.env_import_not_store",
            "environment import is a transient explicit materialisation path, not a credential store",
        ))
    }

    fn materialise(
        &self,
        credential_ref: &CredentialRef,
        class: SecretMaterialisationClass,
    ) -> Result<Option<SecretValue>> {
        if credential_ref != &self.credential_ref || class != SecretMaterialisationClass::ProcessEnv
        {
            return Ok(None);
        }
        self.value
            .as_ref()
            .map(|value| SecretValue::new(value.expose()))
            .transpose()
    }
}

fn read_dotenv_value(path: &Path, name: &str) -> Result<Option<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(provider_error(
                "credential.project_env_unreadable",
                format!("could not read {}: {error}", path.display()),
            ))
        }
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != name {
            continue;
        }
        let value = raw_value.trim();
        let unquoted = if value.len() >= 2
            && ((value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\'')))
        {
            &value[1..value.len() - 1]
        } else {
            value
        };
        return Ok(Some(unquoted.to_string()));
    }
    Ok(None)
}

#[cfg(target_os = "linux")]
mod encrypted_fallback {
    use super::*;
    use argon2::Argon2;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use chacha20poly1305::{
        aead::{Aead, Payload},
        KeyInit, XChaCha20Poly1305, XNonce,
    };
    use serde::{Deserialize, Serialize};
    use std::os::unix::fs::PermissionsExt;

    const FALLBACK_VERSION: &str = "aikit.encrypted-credential/v1";

    #[derive(Debug, Serialize, Deserialize)]
    struct EncryptedCredentialEnvelope {
        version: String,
        credential_ref: CredentialRef,
        salt: String,
        nonce: String,
        ciphertext: String,
    }

    /// Linux-only encrypted fallback used only when Secret Service is absent and
    /// the operator explicitly chooses local encrypted storage.
    pub struct LinuxEncryptedFallbackProvider {
        root: PathBuf,
        passphrase: SecretValue,
        headless_capable: bool,
    }

    impl LinuxEncryptedFallbackProvider {
        pub fn new(
            root: impl Into<PathBuf>,
            passphrase: SecretValue,
            headless_capable: bool,
        ) -> Self {
            Self {
                root: root.into(),
                passphrase,
                headless_capable,
            }
        }

        fn path(&self, credential_ref: &CredentialRef) -> PathBuf {
            let digest = blake3_digest(credential_ref.as_str().as_bytes());
            self.root.join(format!("{digest}.json"))
        }

        fn state(&self, credential_ref: &CredentialRef) -> CredentialBindingState {
            let path = self.path(credential_ref);
            let mut metadata = BTreeMap::new();
            metadata.insert("cipher".into(), "xchacha20-poly1305".into());
            metadata.insert("kdf".into(), "argon2id".into());
            CredentialBindingState {
                credential_ref: credential_ref.clone(),
                provider_ref: SecretProviderRef::new("provider:linux-encrypted-fallback")
                    .expect("static provider ref is valid"),
                provider_tier: SecretProviderTier::ExplicitEncryptedFallback,
                materialisation: SecretMaterialisationClass::ProviderNativeLease,
                binding_provenance: format!("encrypted-file:{}", path.display()),
                revision_or_lease_class: Some(FALLBACK_VERSION.into()),
                expires_at: None,
                revoked: false,
                metadata,
            }
        }

        fn derive_key(&self, salt: &[u8]) -> Result<[u8; 32]> {
            let mut key = [0u8; 32];
            Argon2::default()
                .hash_password_into(self.passphrase.expose().as_bytes(), salt, &mut key)
                .map_err(|error| {
                    provider_error(
                        "credential.encrypted_fallback_kdf_failed",
                        format!("could not derive fallback encryption key: {error}"),
                    )
                })?;
            Ok(key)
        }

        fn read_secret(&self, credential_ref: &CredentialRef) -> Result<Option<SecretValue>> {
            let path = self.path(credential_ref);
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(error) => {
                    return Err(provider_error(
                        "credential.encrypted_fallback_read_failed",
                        format!("could not read {}: {error}", path.display()),
                    ))
                }
            };
            let envelope: EncryptedCredentialEnvelope =
                serde_json::from_slice(&bytes).map_err(|error| {
                    provider_error(
                        "credential.encrypted_fallback_invalid",
                        format!("invalid encrypted fallback envelope: {error}"),
                    )
                })?;
            if envelope.version != FALLBACK_VERSION || envelope.credential_ref != *credential_ref {
                return Err(provider_error(
                    "credential.encrypted_fallback_invalid",
                    "encrypted fallback envelope identity does not match the requested credential",
                ));
            }
            let salt = STANDARD.decode(envelope.salt).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    format!("invalid salt: {error}"),
                )
            })?;
            let nonce = STANDARD.decode(envelope.nonce).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    format!("invalid nonce: {error}"),
                )
            })?;
            if nonce.len() != 24 {
                return Err(provider_error(
                    "credential.encrypted_fallback_invalid",
                    "encrypted fallback nonce must be 24 bytes",
                ));
            }
            let ciphertext = STANDARD.decode(envelope.ciphertext).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    format!("invalid ciphertext: {error}"),
                )
            })?;
            let key = self.derive_key(&salt)?;
            let cipher = XChaCha20Poly1305::new((&key).into());
            let nonce = XNonce::try_from(nonce.as_slice()).map_err(|_| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    "encrypted fallback nonce must be 24 bytes",
                )
            })?;
            let plaintext = cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &ciphertext,
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )
                .map_err(|_| {
                    provider_error(
                        "credential.encrypted_fallback_decrypt_failed",
                        "could not decrypt fallback credential (wrong passphrase or damaged file)",
                    )
                })?;
            let secret = String::from_utf8(plaintext).map_err(|_| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    "decrypted fallback credential is not UTF-8",
                )
            })?;
            SecretValue::new(secret).map(Some)
        }
    }

    impl SecretProvider for LinuxEncryptedFallbackProvider {
        fn descriptor(&self, credential_ref: &CredentialRef) -> SecretProviderDescriptor {
            let bound = self.path(credential_ref).is_file();
            SecretProviderDescriptor {
                provider_ref: SecretProviderRef::new("provider:linux-encrypted-fallback")
                    .expect("static provider ref is valid"),
                provider_kind: "linux-encrypted-fallback".into(),
                tier: SecretProviderTier::ExplicitEncryptedFallback,
                available: true,
                headless_capable: self.headless_capable,
                assurance:
                    "local ciphertext encrypted with Argon2id-derived XChaCha20-Poly1305 key".into(),
                degradation: Some(
                    "used only by explicit operator choice when Secret Service is unavailable"
                        .into(),
                ),
                supported_credentials: if bound {
                    [credential_ref.clone()].into_iter().collect()
                } else {
                    BTreeSet::new()
                },
                supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                    .into_iter()
                    .collect(),
                binding_provenance: format!(
                    "encrypted-file:{}",
                    self.path(credential_ref).display()
                ),
                revision_or_lease_class: Some(FALLBACK_VERSION.into()),
            }
        }

        fn binding_state(
            &self,
            credential_ref: &CredentialRef,
        ) -> Result<Option<CredentialBindingState>> {
            Ok(self
                .path(credential_ref)
                .is_file()
                .then(|| self.state(credential_ref)))
        }

        fn bind(
            &self,
            credential_ref: &CredentialRef,
            secret: &SecretValue,
        ) -> Result<CredentialBindingState> {
            std::fs::create_dir_all(&self.root).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_write_failed",
                    format!("could not create {}: {error}", self.root.display()),
                )
            })?;
            std::fs::set_permissions(&self.root, std::fs::Permissions::from_mode(0o700)).map_err(
                |error| {
                    provider_error(
                        "credential.encrypted_fallback_write_failed",
                        format!("could not restrict {}: {error}", self.root.display()),
                    )
                },
            )?;

            let mut salt = [0u8; 16];
            let mut nonce = [0u8; 24];
            getrandom::fill(&mut salt).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_random_failed",
                    format!("could not generate KDF salt: {error}"),
                )
            })?;
            getrandom::fill(&mut nonce).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_random_failed",
                    format!("could not generate encryption nonce: {error}"),
                )
            })?;
            let key = self.derive_key(&salt)?;
            let cipher = XChaCha20Poly1305::new((&key).into());
            let nonce_value = XNonce::try_from(nonce.as_slice()).expect("fixed 24-byte nonce");
            let ciphertext = cipher
                .encrypt(
                    &nonce_value,
                    Payload {
                        msg: secret.expose().as_bytes(),
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )
                .map_err(|_| {
                    provider_error(
                        "credential.encrypted_fallback_encrypt_failed",
                        "could not encrypt fallback credential",
                    )
                })?;
            let envelope = EncryptedCredentialEnvelope {
                version: FALLBACK_VERSION.into(),
                credential_ref: credential_ref.clone(),
                salt: STANDARD.encode(salt),
                nonce: STANDARD.encode(nonce),
                ciphertext: STANDARD.encode(ciphertext),
            };
            let bytes = serde_json::to_vec_pretty(&envelope).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_write_failed",
                    format!("could not encode encrypted fallback: {error}"),
                )
            })?;
            let path = self.path(credential_ref);
            std::fs::write(&path, bytes).map_err(|error| {
                provider_error(
                    "credential.encrypted_fallback_write_failed",
                    format!("could not write {}: {error}", path.display()),
                )
            })?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
                |error| {
                    provider_error(
                        "credential.encrypted_fallback_write_failed",
                        format!("could not restrict {}: {error}", path.display()),
                    )
                },
            )?;
            Ok(self.state(credential_ref))
        }

        fn materialise(
            &self,
            credential_ref: &CredentialRef,
            class: SecretMaterialisationClass,
        ) -> Result<Option<SecretValue>> {
            if class != SecretMaterialisationClass::ProviderNativeLease {
                return Ok(None);
            }
            self.read_secret(credential_ref)
        }
    }

    fn blake3_digest(bytes: &[u8]) -> String {
        // Keep the fallback filename opaque without adding credential material to
        // a path. This tiny local formatter avoids making blake3 a public seam.
        let digest = blake3::hash(bytes);
        digest.to_hex().to_string()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn fallback_file_contains_ciphertext_not_raw_secret() {
            let temp = tempfile::tempdir().unwrap();
            let credential = CredentialRef::new("credential:test/fallback").unwrap();
            let provider = LinuxEncryptedFallbackProvider::new(
                temp.path(),
                SecretValue::new("fallback-passphrase").unwrap(),
                false,
            );
            let raw = "sk-fixture-never-plaintext";
            provider
                .bind(&credential, &SecretValue::new(raw).unwrap())
                .unwrap();
            let path = provider.path(&credential);
            let on_disk = std::fs::read_to_string(path).unwrap();
            assert!(!on_disk.contains(raw));
            let materialised = provider
                .materialise(&credential, SecretMaterialisationClass::ProviderNativeLease)
                .unwrap()
                .unwrap();
            assert_eq!(materialised.expose(), raw);
        }
    }
}

#[cfg(target_os = "linux")]
pub use encrypted_fallback::LinuxEncryptedFallbackProvider;

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::credential::{
        resolve_registered_credential, CredentialProviderRejection, SecretRequirement,
        SecretRequirementRef,
    };

    fn requirement(credential_ref: CredentialRef) -> SecretRequirement {
        SecretRequirement {
            requirement_ref: SecretRequirementRef::new("secret-requirement:test").unwrap(),
            credential_ref,
            consumer_ref: "harness:test".into(),
            purpose: "test provider authentication".into(),
            permitted_materialisation: [
                SecretMaterialisationClass::ProviderNativeLease,
                SecretMaterialisationClass::ProcessEnv,
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn env_import_exists_but_is_refused_without_explicit_intent() {
        let credential = CredentialRef::new("credential:test/env").unwrap();
        let env = EnvironmentImportProvider::from_value(
            credential.clone(),
            "AIKIT_TEST_TOKEN",
            Some("fixture-token".into()),
        )
        .unwrap();
        let result =
            resolve_registered_credential(requirement(credential), &[&env], true, false).unwrap();
        assert!(!result.selected());
        assert_eq!(
            result.provider_explanations[0].rejection,
            Some(CredentialProviderRejection::EnvironmentImportNotExplicitlyAllowed)
        );
    }

    #[test]
    fn explicit_env_import_stays_lowest_tier() {
        let credential = CredentialRef::new("credential:test/env").unwrap();
        let env = EnvironmentImportProvider::from_value(
            credential.clone(),
            "AIKIT_TEST_TOKEN",
            Some("fixture-token".into()),
        )
        .unwrap();
        let result =
            resolve_registered_credential(requirement(credential), &[&env], true, true).unwrap();
        assert_eq!(
            result.selected_provider_tier,
            Some(SecretProviderTier::ExplicitEnvironmentImport)
        );
    }

    #[test]
    fn dotenv_is_read_only_from_named_explicit_source() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(".env");
        std::fs::write(&path, "OTHER=nope\nAIKIT_TEST_TOKEN='fixture-token'\n").unwrap();
        let credential = CredentialRef::new("credential:test/project-env").unwrap();
        let env = EnvironmentImportProvider::from_process(
            credential.clone(),
            "AIKIT_TEST_TOKEN",
            Some(&path),
        )
        .unwrap();
        let state = env.binding_state(&credential).unwrap().unwrap();
        assert_eq!(
            state.provider_tier,
            SecretProviderTier::ExplicitEnvironmentImport
        );
        assert!(state.binding_provenance.contains("project-env:"));
    }
}
