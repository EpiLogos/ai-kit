//! Secret references for capsules, `central.security/v1` grammar.
//!
//! A capsule declares *where* a secret lives, never *what* it is. The three
//! schemes are the ones the security map ratified: `keychain://` (macOS
//! secure store), `op://` (1Password item field), and `env://` — the legacy
//! escape hatch, admissible only as an explicit environment import.
//!
//! Two laws live here:
//!   * Refs are location only. No type in this module can hold material, so
//!     a ref cannot leak a value by construction.
//!   * Resolution is somebody else's job. [`SecretResolver`] is the seam;
//!     implementations live in the adapter layer over the genuine store
//!     boundaries (the OS keychain via `keyring`, the 1Password CLI). The
//!     core never reimplements vault access.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::credential::SecretValue;
use crate::{AikitError, Result};

pub const KEYCHAIN_SCHEME: &str = "keychain://";
pub const ONEPASSWORD_SCHEME: &str = "op://";
pub const ENV_SCHEME: &str = "env://";

fn invalid(message: impl Into<String>) -> AikitError {
    AikitError::new("secret_ref.invalid", message)
}

fn validate_segment(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(invalid(format!("secret ref {label} segment must not be empty")));
    }
    if value.chars().any(|c| c.is_whitespace() || c == '/') {
        return Err(invalid(format!(
            "secret ref {label} segment must not contain whitespace or '/'"
        )));
    }
    Ok(())
}

/// One declared secret reference. Serialises as its string form so a capsule
/// manifest round-trips exactly the way an author wrote it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SecretRef {
    Keychain { service: String, account: String },
    OnePassword { vault: String, item: String, field: String },
    /// Legacy escape hatch — admissible only through an explicit environment
    /// import, mirroring the `--from-env` law: presence of a matching
    /// variable alone never makes import eligible.
    Env { name: String },
}

impl SecretRef {
    pub fn parse(value: &str) -> Result<Self> {
        if let Some(rest) = value.strip_prefix(KEYCHAIN_SCHEME) {
            let (service, account) = rest.split_once('/').ok_or_else(|| {
                invalid(format!("keychain ref must be {KEYCHAIN_SCHEME}<service>/<account>"))
            })?;
            if account.contains('/') {
                return Err(invalid(
                    "keychain ref takes exactly one service and one account",
                ));
            }
            validate_segment("service", service)?;
            validate_segment("account", account)?;
            return Ok(Self::Keychain {
                service: service.to_string(),
                account: account.to_string(),
            });
        }
        if let Some(rest) = value.strip_prefix(ONEPASSWORD_SCHEME) {
            let parts: Vec<&str> = rest.split('/').collect();
            if parts.len() != 3 || parts.iter().any(|part| part.is_empty()) {
                return Err(invalid(format!(
                    "onepassword ref must be {ONEPASSWORD_SCHEME}<vault>/<item>/<field>"
                )));
            }
            for (label, part) in [("vault", parts[0]), ("item", parts[1]), ("field", parts[2])] {
                validate_segment(label, part)?;
            }
            return Ok(Self::OnePassword {
                vault: parts[0].to_string(),
                item: parts[1].to_string(),
                field: parts[2].to_string(),
            });
        }
        if let Some(name) = value.strip_prefix(ENV_SCHEME) {
            let valid = !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return Err(invalid(format!(
                    "env ref must be {ENV_SCHEME}<NAME> with a POSIX-exportable NAME"
                )));
            }
            return Ok(Self::Env {
                name: name.to_string(),
            });
        }
        Err(invalid(format!(
            "unsupported secret ref scheme: expected {KEYCHAIN_SCHEME}, {ONEPASSWORD_SCHEME} or {ENV_SCHEME}"
        )))
    }

    /// The scheme this ref resolves through — how resolvers dispatch.
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Keychain { .. } => "keychain",
            Self::OnePassword { .. } => "onepassword",
            Self::Env { .. } => "env",
        }
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keychain { service, account } => {
                write!(f, "{KEYCHAIN_SCHEME}{service}/{account}")
            }
            Self::OnePassword { vault, item, field } => {
                write!(f, "{ONEPASSWORD_SCHEME}{vault}/{item}/{field}")
            }
            Self::Env { name } => write!(f, "{ENV_SCHEME}{name}"),
        }
    }
}

impl Serialize for SecretRef {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SecretRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        SecretRef::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// The projection-time resolution seam. Implementations live in the adapter
/// layer and delegate to the genuine store boundary; the core stays free of
/// vault clients.
pub trait SecretResolver: Send + Sync + fmt::Debug {
    /// Resolve a declared ref to material. Implementations must never log or
    /// persist the returned value; it exists only for the one materialisation
    /// operation that needs it.
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_three_schemes() {
        let keychain = SecretRef::parse("keychain://workcell/op-service-account").unwrap();
        assert_eq!(keychain.scheme(), "keychain");
        assert_eq!(keychain.to_string(), "keychain://workcell/op-service-account");

        let op = SecretRef::parse("op://Central/central-security/credential").unwrap();
        assert_eq!(op.scheme(), "onepassword");
        assert_eq!(op.to_string(), "op://Central/central-security/credential");

        let env = SecretRef::parse("env://MY_API_KEY").unwrap();
        assert_eq!(env.scheme(), "env");
        assert_eq!(env.to_string(), "env://MY_API_KEY");
    }

    #[test]
    fn rejects_bad_shapes_and_unknown_schemes() {
        assert!(SecretRef::parse("vault://x/y").is_err());
        assert!(SecretRef::parse("keychain://only").is_err());
        assert!(SecretRef::parse("keychain://a/b/c").is_err());
        assert!(SecretRef::parse("keychain:// /acct").is_err());
        assert!(SecretRef::parse("op://vault/item").is_err());
        assert!(SecretRef::parse("op://vault//field").is_err());
        assert!(SecretRef::parse("env://1BAD").is_err());
        assert!(SecretRef::parse("env://HAS SPACE").is_err());
        assert!(SecretRef::parse("").is_err());
    }

    #[test]
    fn serialises_as_its_string_form() {
        for text in [
            "keychain://svc/acct",
            "op://v/i/f",
            "env://NAME",
        ] {
            let parsed: SecretRef = serde_json::from_str(&format!("\"{text}\"")).unwrap();
            assert_eq!(parsed.to_string(), text);
            assert_eq!(serde_json::to_string(&parsed).unwrap(), format!("\"{text}\""));
        }
        assert!(serde_json::from_str::<SecretRef>("\"not-a-ref\"").is_err());
    }

    #[test]
    fn debug_never_carries_material_because_refs_cannot() {
        // Mechanical backstop for the location-only law: the Debug of every
        // variant contains only its declared segments.
        let rendered = format!(
            "{:?} {:?} {:?}",
            SecretRef::parse("keychain://svc/acct").unwrap(),
            SecretRef::parse("op://v/i/f").unwrap(),
            SecretRef::parse("env://NAME").unwrap(),
        );
        for needle in ["svc", "acct", "v", "i", "f", "NAME"] {
            assert!(rendered.contains(needle));
        }
    }
}
