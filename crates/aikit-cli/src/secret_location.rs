//! Where an unattended credential lives, named by location only.
//!
//! Two unattended consumers need a bearer credential without a person at the
//! keyboard: the gateway relaying a Communique to another Workcell's gateway,
//! and a native Routine run calling a token-gated owner Action (Central's
//! `central.day.ensure`). Both store a *location*, never material:
//!
//! ```text
//! file:/abs/path        an owner-only file (mode 0600 or tighter) — the
//!                       convention Central itself uses for native tokens
//! keychain://… op://… pass://… varlock://…
//!                       a declared secret ref, resolved by the same resolver
//!                       suite every credential consumer uses
//! ```
//!
//! The file form exists because an unattended 00:00 run cannot answer a
//! keychain or gpg prompt; it is refused unless the file is owner-only, so a
//! world-readable token is never silently accepted. `env://` is refused here
//! exactly as it is at `aikit credential setup`: an ambient variable is not a
//! store. Material is returned as [`SecretValue`] (redacted in Debug, never
//! serialisable) and lives only as long as the one child process that needs it.

use std::path::PathBuf;

use aikit_core::credential::SecretValue;
use aikit_core::secret_ref::{SecretRef, SecretResolver as _};
use aikit_core::{AikitError, Result};

/// Refuse anything longer: a bearer token is not a document.
const MAX_SECRET_FILE_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretLocation {
    File(PathBuf),
    Declared(SecretRef),
}

impl SecretLocation {
    pub fn parse(raw: &str) -> Result<Self> {
        let raw = raw.trim();
        if let Some(path) = raw.strip_prefix("file:") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(AikitError::new(
                    "secret_location.invalid",
                    format!("file: locations must be absolute paths, got {raw}"),
                ));
            }
            return Ok(Self::File(path));
        }
        let secret_ref = SecretRef::parse(raw)?;
        if matches!(secret_ref, SecretRef::Env { .. }) {
            return Err(AikitError::new(
                "secret_location.env_refused",
                "env:// names a transient process variable, not a store; declare a file: \
                 location or a keychain/pass/op/varlock ref",
            ));
        }
        Ok(Self::Declared(secret_ref))
    }

    /// The location as it is stored and displayed — never material.
    pub fn render(&self) -> String {
        match self {
            Self::File(path) => format!("file:{}", path.display()),
            Self::Declared(secret_ref) => secret_ref.to_string(),
        }
    }

    /// Materialise at the one moment of use.
    pub fn resolve(&self) -> Result<SecretValue> {
        match self {
            Self::File(path) => read_owner_only(path),
            Self::Declared(secret_ref) => {
                aikit_adapters::secret_resolver::SuiteSecretResolver::default().resolve(secret_ref)
            }
        }
    }
}

fn read_owner_only(path: &std::path::Path) -> Result<SecretValue> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        AikitError::new(
            "secret_location.unreadable",
            format!("credential file {} cannot be read: {error}", path.display()),
        )
        .with("location", format!("file:{}", path.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        if mode & 0o077 != 0 {
            return Err(AikitError::new(
                "secret_location.permissions_too_open",
                format!(
                    "credential file {} has mode {:o}; it must be readable by its owner only \
                     (chmod 600 {})",
                    path.display(),
                    mode & 0o777,
                    path.display()
                ),
            ));
        }
    }
    if metadata.len() > MAX_SECRET_FILE_BYTES {
        return Err(AikitError::new(
            "secret_location.too_large",
            format!(
                "credential file {} is {} bytes; a bearer credential is at most {MAX_SECRET_FILE_BYTES}",
                path.display(),
                metadata.len()
            ),
        ));
    }
    let text = std::fs::read_to_string(path).map_err(|error| {
        AikitError::new(
            "secret_location.unreadable",
            format!("credential file {} cannot be read: {error}", path.display()),
        )
    })?;
    SecretValue::new(text.trim().to_owned()).map_err(|_| {
        AikitError::new(
            "secret_location.empty",
            format!("credential file {} is empty", path.display()),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_world_readable_token_file_is_refused_and_an_owner_only_one_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("token");
        std::fs::write(&path, "  sekrit-token-value \n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let location = SecretLocation::parse(&format!("file:{}", path.display())).unwrap();
            assert_eq!(
                location.resolve().unwrap_err().code(),
                "secret_location.permissions_too_open"
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert_eq!(location.resolve().unwrap().expose(), "sekrit-token-value");
            assert!(!format!("{:?}", location.resolve().unwrap()).contains("sekrit"));
        }
    }

    #[test]
    fn relative_files_and_env_refs_are_not_locations() {
        assert_eq!(
            SecretLocation::parse("file:relative/token")
                .unwrap_err()
                .code(),
            "secret_location.invalid"
        );
        assert_eq!(
            SecretLocation::parse("env://CENTRAL_NATIVE_TOKEN")
                .unwrap_err()
                .code(),
            "secret_location.env_refused"
        );
        let declared = SecretLocation::parse("keychain://central/day-routine").unwrap();
        assert_eq!(declared.render(), "keychain://central/day-routine");
    }
}
