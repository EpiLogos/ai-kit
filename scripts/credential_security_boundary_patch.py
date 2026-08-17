#!/usr/bin/env python3
"""Temporary cloud codemod: ensure availability checks never retrieve secrets."""
from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    target = Path(path)
    text = target.read_text()
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"expected security-boundary anchor missing in {path}: {old[:120]!r}")
    target.write_text(text.replace(old, new, 1))


replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """pub struct NativeSecureStoreProvider;\n\nimpl NativeSecureStoreProvider {\n    pub fn new() -> Self {\n        Self\n    }\n\n    fn entry(credential_ref: &CredentialRef) -> std::result::Result<Entry, KeyringError> {\n        Entry::new(AIKIT_KEYRING_SERVICE, credential_ref.as_str())\n    }\n\n    pub fn status(&self, credential_ref: &CredentialRef) -> NativeSecureStoreStatus {\n        let Ok(entry) = Self::entry(credential_ref) else {\n            return NativeSecureStoreStatus::Unavailable;\n        };\n        match entry.get_password() {\n            Ok(_) => NativeSecureStoreStatus::Bound,\n            Err(KeyringError::NoEntry) => NativeSecureStoreStatus::Unbound,\n            Err(_) => NativeSecureStoreStatus::Unavailable,\n        }\n    }""",
    """pub struct NativeSecureStoreProvider {\n    bound_credentials: BTreeSet<CredentialRef>,\n}\n\nimpl NativeSecureStoreProvider {\n    pub fn new() -> Self {\n        Self {\n            bound_credentials: BTreeSet::new(),\n        }\n    }\n\n    /// Construct from safe binding metadata. Checking whether a credential is\n    /// bound must never materialise the secret from the native store.\n    pub fn with_binding(binding: Option<&CredentialBindingState>) -> Self {\n        let mut provider = Self::new();\n        if let Some(binding) = binding.filter(|binding| {\n            binding.provider_tier == SecretProviderTier::OsSecureStore\n                && binding.provider_ref.as_str() == native_provider_ref()\n                && !binding.revoked\n        }) {\n            provider\n                .bound_credentials\n                .insert(binding.credential_ref.clone());\n        }\n        provider\n    }\n\n    fn entry(credential_ref: &CredentialRef) -> std::result::Result<Entry, KeyringError> {\n        Entry::new(AIKIT_KEYRING_SERVICE, credential_ref.as_str())\n    }\n\n    pub fn status(&self, credential_ref: &CredentialRef) -> NativeSecureStoreStatus {\n        // Initialising an Entry determines whether the platform backend exists,\n        // but deliberately does not retrieve credential material. Binding\n        // presence comes from AIKit's provider-neutral metadata record.\n        if Self::entry(credential_ref).is_err() {\n            return NativeSecureStoreStatus::Unavailable;\n        }\n        if self.bound_credentials.contains(credential_ref) {\n            NativeSecureStoreStatus::Bound\n        } else {\n            NativeSecureStoreStatus::Unbound\n        }\n    }""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """pub fn inspect(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialInspection> {\n    let native = NativeSecureStoreProvider::new();\n    let native_descriptor = native.descriptor(&request.credential);""",
    """pub fn inspect(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialInspection> {\n    let persisted_binding = CredentialBindingStore::new(home).load(&request.credential)?;\n    let native = NativeSecureStoreProvider::with_binding(persisted_binding.as_ref());\n    let native_descriptor = native.descriptor(&request.credential);""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """    let persisted_binding = CredentialBindingStore::new(home).load(&request.credential)?;\n    Ok(CredentialInspection {\n        resolution,\n        persisted_binding,""",
    """    Ok(CredentialInspection {\n        resolution,\n        persisted_binding,""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """    let native = NativeSecureStoreProvider::new();\n    let binding = if selected.as_str().starts_with(\"provider:os-secure-store/\") {\n        native.binding_state(&request.credential)?.ok_or_else(|| {""",
    """    let stored_binding = CredentialBindingStore::new(home).load(&request.credential)?;\n    let native = NativeSecureStoreProvider::with_binding(stored_binding.as_ref());\n    let binding = if selected.as_str().starts_with(\"provider:os-secure-store/\") {\n        native.binding_state(&request.credential)?.ok_or_else(|| {""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """    let resolution = resolve_registered_credential(\n        requirement(request)?,\n        &[&native],\n        false,\n        false,\n    )?;""",
    """    let rebound_native = NativeSecureStoreProvider::with_binding(Some(&binding));\n    let resolution = resolve_registered_credential(\n        requirement(request)?,\n        &[&rebound_native],\n        false,\n        false,\n    )?;""",
)

# Make env presence discoverable without reading a raw value unless the operator
# has explicitly selected the import path.
replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """pub struct EnvironmentImportProvider {\n    credential_ref: CredentialRef,\n    env_var: String,\n    value: Option<SecretValue>,\n    provenance: String,\n}""",
    """pub struct EnvironmentImportProvider {\n    credential_ref: CredentialRef,\n    env_var: String,\n    source_available: bool,\n    value: Option<SecretValue>,\n    provenance: String,\n}""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """impl EnvironmentImportProvider {\n    pub fn from_process(""",
    """impl EnvironmentImportProvider {\n    /// Discover a named environment source without importing its secret value.\n    pub fn discover(\n        credential_ref: CredentialRef,\n        env_var: impl Into<String>,\n        project_env: Option<&Path>,\n    ) -> Result<Self> {\n        let env_var = env_var.into();\n        if env_var.trim().is_empty() {\n            return Err(provider_error(\n                \"credential.env_var_invalid\",\n                \"environment variable name must not be empty\",\n            ));\n        }\n        let shell_available = std::env::var_os(&env_var).is_some();\n        let project_available = project_env\n            .map(|path| dotenv_contains_key(path, &env_var))\n            .transpose()?\n            .unwrap_or(false);\n        let provenance = if shell_available {\n            format!(\"shell-env:{env_var}\")\n        } else if project_available {\n            format!(\n                \"project-env:{}#{env_var}\",\n                project_env.expect(\"project path is present\").display()\n            )\n        } else {\n            format!(\"shell-env:{env_var}\")\n        };\n        Ok(Self {\n            credential_ref,\n            env_var,\n            source_available: shell_available || project_available,\n            value: None,\n            provenance,\n        })\n    }\n\n    pub fn from_process(""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """                    env_var,\n                    value: Some(SecretValue::new(value)?),\n                });""",
    """                    env_var,\n                    source_available: true,\n                    value: Some(SecretValue::new(value)?),\n                });""",
)
# Same field sequence occurs a second time for project .env.
replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """                    env_var,\n                    value: Some(SecretValue::new(value)?),\n                });""",
    """                    env_var,\n                    source_available: true,\n                    value: Some(SecretValue::new(value)?),\n                });""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """            env_var,\n            value: None,\n        })""",
    """            env_var,\n            source_available: false,\n            value: None,\n        })""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """        let value = value.map(SecretValue::new).transpose()?;\n        Ok(Self {\n            credential_ref,\n            provenance: format!(\"shell-env:{env_var}\"),\n            env_var,\n            value,\n        })""",
    """        let source_available = value.is_some();\n        let value = value.map(SecretValue::new).transpose()?;\n        Ok(Self {\n            credential_ref,\n            provenance: format!(\"shell-env:{env_var}\"),\n            env_var,\n            source_available,\n            value,\n        })""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """        let bound = credential_ref == &self.credential_ref && self.value.is_some();""",
    """        let bound = credential_ref == &self.credential_ref && self.source_available;""",
)

replace_once(
    "crates/aikit-adapters/src/credential_provider.rs",
    """fn read_dotenv_value(path: &Path, name: &str) -> Result<Option<String>> {""",
    """fn dotenv_contains_key(path: &Path, name: &str) -> Result<bool> {\n    let text = match std::fs::read_to_string(path) {\n        Ok(text) => text,\n        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),\n        Err(error) => {\n            return Err(provider_error(\n                \"credential.project_env_unreadable\",\n                format!(\"could not read {}: {error}\", path.display()),\n            ))\n        }\n    };\n    Ok(text.lines().any(|raw| {\n        let line = raw.trim();\n        if line.is_empty() || line.starts_with('#') {\n            return false;\n        }\n        let line = line.strip_prefix(\"export \").unwrap_or(line).trim();\n        line.split_once('=')\n            .map(|(key, _)| key.trim() == name)\n            .unwrap_or(false)\n    }))\n}\n\nfn read_dotenv_value(path: &Path, name: &str) -> Result<Option<String>> {""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """    let env = request\n        .env_var\n        .as_ref()\n        .map(|env_var| {\n            EnvironmentImportProvider::from_process(\n                request.credential.clone(),\n                env_var.clone(),\n                request.project_env.as_deref(),\n            )\n        })\n        .transpose()?;""",
    """    let env = request\n        .env_var\n        .as_ref()\n        .map(|env_var| {\n            if request.from_env {\n                EnvironmentImportProvider::from_process(\n                    request.credential.clone(),\n                    env_var.clone(),\n                    request.project_env.as_deref(),\n                )\n            } else {\n                EnvironmentImportProvider::discover(\n                    request.credential.clone(),\n                    env_var.clone(),\n                    request.project_env.as_deref(),\n                )\n            }\n        })\n        .transpose()?;""",
)

replace_once(
    "crates/aikit-cli/src/credential.rs",
    """    let env_available = env\n        .as_ref()\n        .and_then(|provider| provider.binding_state(&request.credential).transpose())\n        .transpose()?\n        .is_some();""",
    """    let env_available = env\n        .as_ref()\n        .map(|provider| {\n            provider\n                .descriptor(&request.credential)\n                .supported_credentials\n                .contains(&request.credential)\n        })\n        .unwrap_or(false);""",
)
