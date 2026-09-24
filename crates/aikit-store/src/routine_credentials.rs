//! Where a native Routine run finds the owner credential it needs:
//! `<aikit-home>/state/routine-credentials.json`.
//!
//! A native Method (one whose body is owner Actions, not a model run) may need
//! a bearer credential for exactly one owner Action — Central's token-gated
//! `central.day.ensure` is the first. The Method declares the need (an
//! environment variable name and the Actions it is for); this store binds the
//! need, per Routine, to a *location* (`file:/abs/path` or a declared secret
//! ref). It never holds material: the value is read at the one moment the
//! runner spawns the owner child, and only that child's environment carries it.
//!
//! Why a store beside the Routine rather than a field on it: the binding is
//! machine standing (where this machine keeps its token), not Routine
//! identity. Keeping it out of the Routine body keeps the Routine's
//! content-hash revision — and every admitted invocation bound to it — stable
//! when a token file moves, and keeps binaries that predate native Methods
//! able to read the Routine store unchanged.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};

use crate::{AikitHome, ContextLock, LockOptions};

pub const ROUTINE_CREDENTIALS_VERSION: &str = "aikit.routine-credential-bindings/v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingsFile {
    schema: String,
    /// routine ref -> env var name -> location.
    #[serde(default)]
    bindings: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for BindingsFile {
    fn default() -> Self {
        Self {
            schema: ROUTINE_CREDENTIALS_VERSION.into(),
            bindings: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RoutineCredentialStore {
    home: AikitHome,
}

fn valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_uppercase() || first == '_')
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

impl RoutineCredentialStore {
    pub fn new(home: AikitHome) -> Self {
        Self { home }
    }

    pub fn path(&self) -> PathBuf {
        self.home.state().join("routine-credentials.json")
    }

    /// Every env -> location binding for one Routine.
    pub fn bindings(&self, routine_ref: &ResourceRef) -> Result<BTreeMap<String, String>> {
        Ok(self
            .load()?
            .bindings
            .remove(routine_ref.as_str())
            .unwrap_or_default())
    }

    /// Bind (or with `location == None`, unbind) one variable for a Routine.
    pub fn set(
        &self,
        routine_ref: &ResourceRef,
        env: &str,
        location: Option<&str>,
    ) -> Result<BTreeMap<String, String>> {
        if !valid_env_name(env) {
            return Err(AikitError::new(
                "routine.credential_env_invalid",
                format!("{env:?} is not a credential variable name (A-Z, 0-9, _)"),
            ));
        }
        if location.is_some_and(|location| location.trim().is_empty()) {
            return Err(AikitError::new(
                "routine.credential_location_empty",
                "a credential location must not be empty",
            ));
        }
        let _lock = ContextLock::acquire(
            &self.home,
            "routine-credentials",
            LockOptions::default().with_purpose("bind a Routine credential location"),
        )?;
        let mut file = self.load()?;
        let entry = file.bindings.entry(routine_ref.to_string()).or_default();
        match location {
            Some(location) => {
                entry.insert(env.to_owned(), location.trim().to_owned());
            }
            None => {
                entry.remove(env);
            }
        }
        let current = entry.clone();
        file.bindings.retain(|_, entries| !entries.is_empty());
        self.write(&file)?;
        Ok(current)
    }

    fn load(&self) -> Result<BindingsFile> {
        let path = self.path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BindingsFile::default())
            }
            Err(error) => {
                return Err(AikitError::new(
                    "routine.credentials_read_failed",
                    format!("{}: {error}", path.display()),
                ))
            }
        };
        let file: BindingsFile = serde_json::from_slice(&bytes).map_err(|error| {
            AikitError::new(
                "routine.credentials_invalid",
                format!("{}: {error}", path.display()),
            )
        })?;
        if file.schema != ROUTINE_CREDENTIALS_VERSION {
            return Err(AikitError::new(
                "routine.credentials_invalid",
                format!("{} uses unsupported schema {}", path.display(), file.schema),
            ));
        }
        Ok(file)
    }

    fn write(&self, file: &BindingsFile) -> Result<()> {
        let path = self.path();
        let failed = |error: std::io::Error| {
            AikitError::new(
                "routine.credentials_write_failed",
                format!("{}: {error}", path.display()),
            )
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(failed)?;
        }
        let bytes = serde_json::to_vec_pretty(file).map_err(|error| {
            AikitError::new("routine.credentials_write_failed", error.to_string())
        })?;
        let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
        fs::write(&temporary, bytes).map_err(failed)?;
        fs::rename(&temporary, &path).map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_a_location_per_routine_and_variable_and_unbinding_removes_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = RoutineCredentialStore::new(AikitHome::at(dir.path().join("home")));
        let routine = ResourceRef::parse("routine/central-day-rollover").unwrap();
        let other = ResourceRef::parse("routine/other").unwrap();
        store
            .set(&routine, "CENTRAL_NATIVE_TOKEN", Some("file:/secure/token"))
            .unwrap();
        assert_eq!(
            store
                .bindings(&routine)
                .unwrap()
                .get("CENTRAL_NATIVE_TOKEN")
                .map(String::as_str),
            Some("file:/secure/token")
        );
        assert!(store.bindings(&other).unwrap().is_empty());
        let on_disk = std::fs::read_to_string(store.path()).unwrap();
        assert!(on_disk.contains(ROUTINE_CREDENTIALS_VERSION));
        store.set(&routine, "CENTRAL_NATIVE_TOKEN", None).unwrap();
        assert!(store.bindings(&routine).unwrap().is_empty());
        assert_eq!(
            store
                .set(&routine, "lower", Some("file:/x"))
                .unwrap_err()
                .code(),
            "routine.credential_env_invalid"
        );
    }
}
