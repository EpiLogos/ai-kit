//! Explicit, read-only delivery from the original native Pi auth store.
//! Discovery/declaration/inspection use public object metadata only. Material is
//! read solely by the selected provider at actual delivery, never copied into a
//! Task config directory, binding, receipt or journal.

use std::collections::BTreeMap;
use std::fs::{self, File, Metadata};
use std::io::Read;
use std::path::{Path, PathBuf};

use aikit_core::credential::{
    CredentialBindingState, CredentialRef, HarnessAuthProvider, HarnessAuthSource,
    HarnessAuthSourceObject, SecretMaterialisationClass, SecretProvider, SecretProviderDescriptor,
    SecretProviderRef, SecretProviderTier, SecretValue,
};
use aikit_core::{AikitError, Result};

const PROVIDER: &str = "provider:named-harness-auth/pi/zai";
const REVISION: &str = "pi-native-auth-source/v1";
const MAX_AUTH_BYTES: u64 = 1024 * 1024;

fn refused(message: &str) -> AikitError {
    // Do not attach JSON parsing errors, file contents or key fragments.
    AikitError::new("credential.harness_auth_refused", message)
}

/// Native origin is HOME/.pi/agent/auth.json, independent of the mutable Task's
/// PI_CODING_AGENT_DIR. There is deliberately no environment/path source flag.
pub fn native_home() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| refused("native home is unavailable"))?;
    fs::canonicalize(home).map_err(|_| refused("native home cannot be resolved"))
}

#[cfg(unix)]
fn object(metadata: &Metadata) -> Result<HarnessAuthSourceObject> {
    use std::os::unix::fs::MetadataExt;
    if !metadata.is_file() || metadata.mode() & 0o077 != 0 {
        return Err(refused("Pi auth source must be a private regular file"));
    }
    Ok(HarnessAuthSourceObject {
        device: metadata.dev(),
        inode: metadata.ino(),
        byte_len: metadata.len(),
        modified_seconds: metadata.mtime(),
        modified_nanoseconds: metadata.mtime_nsec(),
        changed_seconds: metadata.ctime(),
        changed_nanoseconds: metadata.ctime_nsec(),
        owner: metadata.uid(),
        mode: metadata.mode(),
    })
}

#[cfg(not(unix))]
fn object(_metadata: &Metadata) -> Result<HarnessAuthSourceObject> {
    Err(refused(
        "named Pi auth source identity is unsupported on this platform",
    ))
}

fn source_object(home: &Path, path: &Path) -> Result<HarnessAuthSourceObject> {
    if path != home.join(".pi/agent/auth.json") {
        return Err(refused("Pi source differs from its native origin"));
    }
    // Canonical equality also refuses symlinked parent directories. The open
    // descriptor is independently checked against this same object at use time.
    if fs::canonicalize(path).map_err(|_| refused("Pi auth source is missing"))? != path {
        return Err(refused("symlinked Pi auth sources are refused"));
    }
    let metadata =
        fs::symlink_metadata(path).map_err(|_| refused("Pi auth source is unavailable"))?;
    let home_metadata = fs::metadata(home).map_err(|_| refused("native home is unavailable"))?;
    let basis = object(&metadata)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if basis.owner != home_metadata.uid() {
            return Err(refused("Pi auth source belongs to another native owner"));
        }
    }
    #[cfg(not(unix))]
    let _ = home_metadata;
    Ok(basis)
}

/// The only backed provider initially supported is Pi's literal zai api_key.
/// Constructors accepting a home are for the owner and real isolated filesystem
/// tests; public CLI requests cannot choose an arbitrary home or source path.
#[derive(Debug)]
pub struct PiHarnessAuthProvider {
    binding: Option<CredentialBindingState>,
    home: PathBuf,
    consumer: String,
}

impl PiHarnessAuthProvider {
    pub fn from_native(binding: Option<&CredentialBindingState>, consumer: &str) -> Result<Self> {
        Self::at(binding, &native_home()?, consumer)
    }

    pub fn at(
        binding: Option<&CredentialBindingState>,
        home: &Path,
        consumer: &str,
    ) -> Result<Self> {
        Ok(Self {
            binding: binding.cloned(),
            home: fs::canonicalize(home).map_err(|_| refused("native home cannot be resolved"))?,
            consumer: consumer.to_string(),
        })
    }

    pub fn declare(
        home: &Path,
        credential: &CredentialRef,
        consumer: &str,
        purpose: &str,
        expires_at: &str,
    ) -> Result<CredentialBindingState> {
        if credential.as_str() != "credential:z-ai"
            || !consumer.starts_with("agent-session/")
            || consumer.trim() != consumer
            || consumer.len() == "agent-session/".len()
            || purpose.trim().is_empty()
        {
            return Err(refused(
                "Pi/zai requires credential:z-ai, an exact agent-session consumer and purpose",
            ));
        }
        let expiry = expires_at
            .parse::<jiff::Timestamp>()
            .map_err(|_| refused("Pi auth declaration needs a finite timestamp expiry"))?;
        if expiry <= jiff::Timestamp::now() {
            return Err(refused("Pi auth declaration expiry has elapsed"));
        }
        let home = fs::canonicalize(home).map_err(|_| refused("native home cannot be resolved"))?;
        let path = home.join(".pi/agent/auth.json");
        let basis = source_object(&home, &path)?;
        Ok(CredentialBindingState {
            credential_ref: credential.clone(),
            provider_ref: SecretProviderRef::new(PROVIDER)?,
            provider_tier: SecretProviderTier::NamedHarnessAuthStore,
            materialisation: SecretMaterialisationClass::ProcessEnv,
            binding_provenance: "explicit native Pi auth source; selected zai entry".into(),
            revision_or_lease_class: Some(REVISION.into()),
            expires_at: Some(expiry.to_string()),
            revoked: false,
            metadata: BTreeMap::new(),
            declared_secret_ref: None,
            harness_auth_source: Some(HarnessAuthSource {
                provider: HarnessAuthProvider::PiZai,
                native_home: home,
                source_path: path,
                object: basis,
                consumer_ref: consumer.to_string(),
                purpose: purpose.to_string(),
            }),
            bound_at_unix_seconds: None,
            last_rotated_at_unix_seconds: None,
            last_verified_at_unix_seconds: None,
        })
    }

    pub fn revalidate(&self, credential: &CredentialRef) -> Result<&CredentialBindingState> {
        let binding = self
            .binding
            .as_ref()
            .ok_or_else(|| refused("Pi auth source is not declared"))?;
        let source = binding
            .harness_auth_source
            .as_ref()
            .ok_or_else(|| refused("Pi auth source is not declared"))?;
        if binding.credential_ref != *credential
            || credential.as_str() != "credential:z-ai"
            || binding.provider_ref.as_str() != PROVIDER
            || binding.provider_tier != SecretProviderTier::NamedHarnessAuthStore
            || binding.materialisation != SecretMaterialisationClass::ProcessEnv
            || binding.declared_secret_ref.is_some()
            || binding.revoked
            || source.provider != HarnessAuthProvider::PiZai
            || source.native_home != self.home
            || source.consumer_ref != self.consumer
            || source.purpose.trim().is_empty()
        {
            return Err(refused(
                "Pi auth binding identity, consumer or backing differs; no fallback",
            ));
        }
        let expiry = binding
            .expires_at
            .as_deref()
            .ok_or_else(|| refused("Pi auth binding has no finite expiry"))?
            .parse::<jiff::Timestamp>()
            .map_err(|_| refused("Pi auth expiry is invalid"))?;
        if expiry <= jiff::Timestamp::now() {
            return Err(refused("Pi auth binding has expired"));
        }
        if source_object(&self.home, &source.source_path)? != source.object {
            return Err(refused(
                "Pi auth source changed; explicit rotation and fresh preparation required",
            ));
        }
        Ok(binding)
    }
}

impl SecretProvider for PiHarnessAuthProvider {
    fn descriptor(&self, credential: &CredentialRef) -> SecretProviderDescriptor {
        let (eligible, refusal) = match self.revalidate(credential) {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.message().to_string())),
        };
        let mut degradation = "Existing plaintext harness store; no OS secure-store assurance or live key-quality claim".to_string();
        if let Some(refusal) = refusal {
            degradation.push_str("; currently refused: ");
            degradation.push_str(&refusal);
        }
        SecretProviderDescriptor {
            provider_ref: SecretProviderRef::new(PROVIDER).expect("static provider reference"),
            provider_kind: "named-native-pi-auth-source".into(),
            tier: SecretProviderTier::NamedHarnessAuthStore,
            available: eligible,
            headless_capable: true,
            assurance:
                "Explicit original private Pi auth file; literal zai api_key read only at delivery"
                    .into(),
            degradation: Some(degradation),
            supported_credentials: if eligible {
                [credential.clone()].into()
            } else {
                Default::default()
            },
            supported_materialisation: [SecretMaterialisationClass::ProcessEnv].into(),
            binding_provenance: "explicit native Pi auth source; selected zai entry".into(),
            revision_or_lease_class: Some(REVISION.into()),
        }
    }

    fn binding_state(&self, credential: &CredentialRef) -> Result<Option<CredentialBindingState>> {
        Ok(Some(self.revalidate(credential)?.clone()))
    }

    fn bind(
        &self,
        _credential: &CredentialRef,
        _secret: &SecretValue,
    ) -> Result<CredentialBindingState> {
        Err(refused(
            "Pi source is read-only; use explicit declaration or rotation",
        ))
    }

    fn materialise(
        &self,
        credential: &CredentialRef,
        class: SecretMaterialisationClass,
    ) -> Result<Option<SecretValue>> {
        if class != SecretMaterialisationClass::ProcessEnv {
            return Err(refused(
                "Pi auth delivery supports only the selected child process environment",
            ));
        }
        let binding = self.revalidate(credential)?;
        let source = binding
            .harness_auth_source
            .as_ref()
            .expect("revalidated typed source");
        let mut file = File::open(&source.source_path)
            .map_err(|_| refused("Pi auth source cannot be opened"))?;
        if object(
            &file
                .metadata()
                .map_err(|_| refused("Pi auth descriptor cannot be inspected"))?,
        )? != source.object
        {
            return Err(refused("Pi auth source changed while opening"));
        }
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_AUTH_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| refused("Pi auth source cannot be read"))?;
        if bytes.len() as u64 > MAX_AUTH_BYTES {
            return Err(refused("Pi auth source exceeds the bounded format limit"));
        }
        if object(
            &file
                .metadata()
                .map_err(|_| refused("Pi auth descriptor cannot be inspected"))?,
        )? != source.object
        {
            return Err(refused("Pi auth source changed while reading"));
        }
        self.revalidate(credential)?;
        let parsed: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| refused("Pi auth source is not valid auth JSON"))?;
        let entry = parsed
            .as_object()
            .and_then(|root| root.get("zai"))
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| refused("Pi auth source has no literal zai api_key entry"))?;
        if entry.get("type").and_then(serde_json::Value::as_str) != Some("api_key")
            || entry.get("env").is_some()
            || entry.keys().any(|k| k != "type" && k != "key")
        {
            return Err(refused(
                "Pi auth delivery refuses OAuth, command, environment and unsupported entries",
            ));
        }
        let key = entry
            .get("key")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| refused("Pi zai api_key must be a literal string"))?;
        if key.trim().is_empty()
            || key.trim() != key
            || key.starts_with('!')
            || key.contains('$')
            || key.chars().any(char::is_control)
        {
            return Err(refused(
                "Pi zai api_key must be a nonempty literal; evaluation is refused",
            ));
        }
        Ok(Some(SecretValue::new(key)?))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn fixture(text: &str) -> (tempfile::TempDir, CredentialBindingState) {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(".pi/agent/auth.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let credential = CredentialRef::new("credential:z-ai").unwrap();
        let binding = PiHarnessAuthProvider::declare(
            home.path(),
            &credential,
            "agent-session/pi-source-test",
            "Real isolated source delivery test",
            "2099-01-01T00:00:00Z",
        )
        .unwrap();
        (home, binding)
    }

    fn provider(home: &Path, binding: &CredentialBindingState) -> PiHarnessAuthProvider {
        PiHarnessAuthProvider::at(Some(binding), home, "agent-session/pi-source-test").unwrap()
    }

    #[test]
    fn declaration_inspection_and_restart_never_parse_or_persist_auth_values() {
        let (home, binding) = fixture("malformed private JSON with synthetic-secret");
        let before = fs::read(home.path().join(".pi/agent/auth.json")).unwrap();
        let persisted = serde_json::to_vec(&binding).unwrap();
        assert!(!String::from_utf8_lossy(&persisted).contains("synthetic-secret"));
        let restored: CredentialBindingState = serde_json::from_slice(&persisted).unwrap();
        let p = provider(home.path(), &restored);
        assert!(p.descriptor(&binding.credential_ref).available);
        let err = p
            .materialise(
                &binding.credential_ref,
                SecretMaterialisationClass::ProcessEnv,
            )
            .unwrap_err();
        assert!(!format!("{err:?}").contains("synthetic-secret"));
        assert_eq!(
            fs::read(home.path().join(".pi/agent/auth.json")).unwrap(),
            before
        );
    }

    #[test]
    fn literal_source_delivers_exact_selected_value_without_touching_source_or_parent_env() {
        let (home, binding) = fixture(
            r#"{"zai":{"type":"api_key","key":"synthetic-private-value"},"other":{"type":"api_key","key":"neighbor"}}"#,
        );
        let path = home.path().join(".pi/agent/auth.json");
        let before = fs::read(&path).unwrap();
        let parent = std::env::var_os("ZAI_API_KEY");
        let p = provider(home.path(), &binding);
        let value = p
            .materialise(
                &binding.credential_ref,
                SecretMaterialisationClass::ProcessEnv,
            )
            .unwrap()
            .unwrap();
        assert_eq!(value.expose(), "synthetic-private-value");
        let env = crate::connection_process::ModelEnvironment::new()
            .with_credential("ZAI_API_KEY", value)
            .unwrap();
        let mut child = std::process::Command::new("/bin/sh");
        child.args(["-c", "test \"$ZAI_API_KEY\" = synthetic-private-value"]);
        env.apply(&mut child);
        assert!(child.status().unwrap().success());
        assert_eq!(std::env::var_os("ZAI_API_KEY"), parent);
        assert_eq!(fs::read(path).unwrap(), before);
        assert!(!format!("{env:?}").contains("synthetic-private-value"));
    }

    #[test]
    fn nonliteral_unsupported_and_missing_entries_refuse_without_value_errors() {
        for text in [
            r#"{"zai":{"type":"oauth","access":"synthetic-private-value"}}"#,
            r#"{"zai":{"type":"api_key","key":"!echo synthetic-private-value"}}"#,
            r#"{"zai":{"type":"api_key","key":"${PRIVATE_KEY}"}}"#,
            r#"{"zai":{"type":"api_key","key":"literal","env":{}}}"#,
            r#"{"zai":{"type":"api_key","key":""}}"#,
            r#"{"zai":{"type":"api_key","key":42}}"#,
            r#"{"neighbor":{"type":"api_key","key":"synthetic-private-value"}}"#,
        ] {
            let (home, binding) = fixture(text);
            let error = provider(home.path(), &binding)
                .materialise(
                    &binding.credential_ref,
                    SecretMaterialisationClass::ProcessEnv,
                )
                .unwrap_err();
            assert!(!format!("{error:?}").contains("synthetic-private-value"));
        }
    }

    #[test]
    fn changed_replaced_missing_and_symlinked_sources_fence_old_binding() {
        let original = r#"{"zai":{"type":"api_key","key":"synthetic-private-value"}}"#;
        for action in 0..4 {
            let (home, binding) = fixture(original);
            let path = home.path().join(".pi/agent/auth.json");
            match action {
                0 => fs::write(&path, "changed source").unwrap(),
                1 => {
                    fs::remove_file(&path).unwrap();
                    fs::write(&path, original).unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                }
                2 => fs::remove_file(&path).unwrap(),
                _ => {
                    fs::rename(&path, path.with_extension("original")).unwrap();
                    symlink(path.with_extension("original"), &path).unwrap();
                }
            }
            let p = provider(home.path(), &binding);
            assert!(!p.descriptor(&binding.credential_ref).available);
            assert!(p
                .materialise(
                    &binding.credential_ref,
                    SecretMaterialisationClass::ProcessEnv
                )
                .is_err());
        }
    }

    #[test]
    fn consumer_revocation_expiry_alias_and_delivery_class_do_not_fallback() {
        let (home, binding) =
            fixture(r#"{"zai":{"type":"api_key","key":"synthetic-private-value"}}"#);
        assert!(
            !PiHarnessAuthProvider::at(Some(&binding), home.path(), "agent-session/neighbor")
                .unwrap()
                .descriptor(&binding.credential_ref)
                .available
        );
        for modified in [
            {
                let mut b = binding.clone();
                b.revoked = true;
                b
            },
            {
                let mut b = binding.clone();
                b.expires_at = Some("2000-01-01T00:00:00Z".into());
                b
            },
            {
                let mut b = binding.clone();
                b.credential_ref = CredentialRef::new("credential:zai").unwrap();
                b
            },
            {
                let mut b = binding.clone();
                b.harness_auth_source.as_mut().unwrap().source_path = home.path().join("other");
                b
            },
        ] {
            assert!(
                !provider(home.path(), &modified)
                    .descriptor(&modified.credential_ref)
                    .available
            );
        }
        assert!(provider(home.path(), &binding)
            .materialise(
                &binding.credential_ref,
                SecretMaterialisationClass::FileOrTmpfsMount
            )
            .is_err());
    }
}
