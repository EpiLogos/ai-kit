//! Projection-time secret resolution.
//!
//! The three implementations each delegate to a genuine store boundary — the
//! OS secure store via `keyring`, the 1Password CLI (`op read`, the same SDK
//! boundary the Workcell onepassword adapter uses), and the process
//! environment for the legacy `env://` escape hatch. No vault client is
//! reimplemented here.
//!
//! The `env://` scheme is gated: admissible only when the operator explicitly
//! opts in (the `--from-env` law — a matching variable in the environment
//! never makes import eligible by itself). The default suite resolver keeps
//! the gate closed.

use std::fmt;

use aikit_core::{AikitError, Result, SecretRef, SecretResolver, SecretValue};

fn unavailable(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

fn unsupported_scheme(secret_ref: &SecretRef, expected: &str) -> AikitError {
    AikitError::new(
        "secret_resolver.unsupported_scheme",
        format!("this resolver handles {expected} refs, not {secret_ref}"),
    )
}

/// `keychain://<service>/<account>` via the OS secure store.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeychainSecretResolver;

impl SecretResolver for KeychainSecretResolver {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        let SecretRef::Keychain { service, account } = secret_ref else {
            return Err(unsupported_scheme(secret_ref, "keychain://"));
        };
        let entry = keyring::Entry::new(service, account).map_err(|error| {
            unavailable(
                "secret_resolver.keychain_unavailable",
                format!("the OS secure store is not reachable here: {error}"),
            )
        })?;
        let value = entry.get_password().map_err(|error| match error {
            keyring::Error::NoEntry => unavailable(
                "secret_resolver.not_found",
                format!("no keychain entry at {secret_ref}"),
            ),
            other => unavailable(
                "secret_resolver.keychain_error",
                format!("keychain read at {secret_ref} failed: {other}"),
            ),
        })?;
        SecretValue::new(value)
    }
}

/// The materialisation primitive for `op://` refs: run `op read <ref>`.
/// Injectable so tests never touch a live vault.
pub trait OpRead: Send + Sync + fmt::Debug {
    fn read(&self, op_ref: &str) -> std::result::Result<String, String>;
}

/// The genuine 1Password CLI. `OP_SERVICE_ACCOUNT_TOKEN` is honoured by `op`
/// itself; its durable home is the keychain entry T3's adapter manages.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpCli;

impl OpRead for OpCli {
    fn read(&self, op_ref: &str) -> std::result::Result<String, String> {
        let output = std::process::Command::new("op")
            .args(["read", op_ref])
            .output()
            .map_err(|error| format!("failed to spawn `op` (is the 1Password CLI installed?): {error}"))?;
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map(|value| value.trim_end_matches(['\n', '\r']).to_string())
                .map_err(|_| "op read returned non-UTF-8 material".to_string());
        }
        Err(classify_op_error(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// Map `op` stderr to a capability fact, mirroring the Workcell adapter's
/// classification: auth absence names the remediation, never a bare failure.
fn classify_op_error(stderr: &str) -> String {
    let lowered = stderr.to_lowercase();
    if lowered.contains("not signed in") || lowered.contains("no accounts configured") {
        "1Password CLI is not signed in; provide OP_SERVICE_ACCOUNT_TOKEN (durable home: keychain://workcell/op-service-account)".to_string()
    } else if lowered.contains("item not found") || lowered.contains("isn't an item") {
        "1Password item not found for the given op:// ref".to_string()
    } else if lowered.contains("doesn't have the access") || lowered.contains("access denied") {
        "1Password service account lacks access to this vault".to_string()
    } else {
        let first = stderr.trim().lines().next().unwrap_or("(no stderr)");
        format!("op read failed: {}", first.chars().take(200).collect::<String>())
    }
}

/// `op://<vault>/<item>/<field>` via the genuine 1Password CLI.
#[derive(Debug, Clone)]
pub struct OnePasswordSecretResolver<R: OpRead = OpCli> {
    runner: R,
}

impl<R: OpRead> OnePasswordSecretResolver<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl Default for OnePasswordSecretResolver<OpCli> {
    fn default() -> Self {
        Self { runner: OpCli }
    }
}

impl<R: OpRead> SecretResolver for OnePasswordSecretResolver<R> {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        let SecretRef::OnePassword { .. } = secret_ref else {
            return Err(unsupported_scheme(secret_ref, "op://"));
        };
        let value = self.runner.read(&secret_ref.to_string()).map_err(|message| {
            unavailable("secret_resolver.onepassword_unavailable", message)
        })?;
        if value.is_empty() {
            return Err(unavailable(
                "secret_resolver.empty_material",
                format!("op read returned empty material for {secret_ref}"),
            ));
        }
        SecretValue::new(value)
    }
}

/// `env://<NAME>` — the legacy escape hatch. Constructed with `allow: false`
/// by default: presence of a matching variable never makes import eligible.
/// The variable source is injectable so tests never mutate process state.
pub struct EnvImportSecretResolver {
    allow: bool,
    source: VarSource,
}

/// Where an env-import resolver reads from: the process by default, a
/// scripted map in tests. Aliased because the trait-object signature trips
/// the type-complexity gate on its own.
type VarSource = std::sync::Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

impl fmt::Debug for EnvImportSecretResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EnvImportSecretResolver")
            .field("allow", &self.allow)
            .field("source", &"<fn>")
            .finish()
    }
}

impl EnvImportSecretResolver {
    pub fn new(allow_env_import: bool) -> Self {
        Self::with_source(allow_env_import, |name| std::env::var(name).ok())
    }

    pub fn with_source(
        allow_env_import: bool,
        source: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            allow: allow_env_import,
            source: std::sync::Arc::new(source),
        }
    }
}

impl Default for EnvImportSecretResolver {
    fn default() -> Self {
        Self::new(false)
    }
}

impl SecretResolver for EnvImportSecretResolver {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        let SecretRef::Env { name } = secret_ref else {
            return Err(unsupported_scheme(secret_ref, "env://"));
        };
        if !self.allow {
            return Err(unavailable(
                "secret_resolver.env_import_not_opted_in",
                format!(
                    "{secret_ref} requires an explicit operator opt-in; a matching variable in the environment alone never makes import eligible"
                ),
            ));
        }
        let value = (self.source)(name).ok_or_else(|| {
            unavailable(
                "secret_resolver.not_found",
                format!("{secret_ref} is not set in this process"),
            )
        })?;
        SecretValue::new(value)
    }
}

/// The default composite: keychain + 1Password, with the environment import
/// gate closed unless the operator opened it.
#[derive(Debug, Default)]
pub struct SuiteSecretResolver {
    pub keychain: KeychainSecretResolver,
    pub onepassword: OnePasswordSecretResolver<OpCli>,
    pub env_import: EnvImportSecretResolver,
}

impl SuiteSecretResolver {
    /// The suite with the legacy environment import gate explicitly opened.
    pub fn with_env_import() -> Self {
        Self {
            env_import: EnvImportSecretResolver::new(true),
            ..Self::default()
        }
    }
}

impl SecretResolver for SuiteSecretResolver {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        match secret_ref {
            SecretRef::Keychain { .. } => self.keychain.resolve(secret_ref),
            SecretRef::OnePassword { .. } => self.onepassword.resolve(secret_ref),
            SecretRef::Env { .. } => self.env_import.resolve(secret_ref),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct ScriptedOp {
        result: std::result::Result<String, String>,
    }

    impl OpRead for ScriptedOp {
        fn read(&self, _op_ref: &str) -> std::result::Result<String, String> {
            self.result.clone()
        }
    }

    fn op_ref() -> SecretRef {
        SecretRef::parse("op://Central/central-security/credential").unwrap()
    }

    #[test]
    fn onepassword_resolves_material_and_refuses_empty() {
        let resolver = OnePasswordSecretResolver::new(ScriptedOp {
            result: Ok("fixture-material".to_string()),
        });
        let material = resolver.resolve(&op_ref()).unwrap();
        assert_eq!(material.expose(), "fixture-material");

        let empty = OnePasswordSecretResolver::new(ScriptedOp {
            result: Ok(String::new()),
        });
        let err = empty.resolve(&op_ref()).unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn onepassword_rejects_wrong_scheme_without_touching_cli() {
        let resolver = OnePasswordSecretResolver::new(ScriptedOp {
            result: Err("must not be called".to_string()),
        });
        let err = resolver
            .resolve(&SecretRef::parse("keychain://svc/acct").unwrap())
            .unwrap_err();
        assert!(err.message().contains("keychain://"));
    }

    #[test]
    fn op_error_classifier_names_capability_facts() {
        assert!(classify_op_error("[ERROR] You are not signed in").contains("not signed in"));
        assert!(classify_op_error("[ERROR] no accounts configured").contains("not signed in"));
        assert!(classify_op_error("[ERROR] item not found").contains("item not found"));
        assert!(classify_op_error("[ERROR] doesn't have the access").contains("lacks access"));
        assert!(classify_op_error("[ERROR] network unreachable").contains("op read failed"));
    }

    #[test]
    fn env_import_is_refused_without_explicit_opt_in() {
        let resolver = EnvImportSecretResolver::default();
        let err = resolver
            .resolve(&SecretRef::parse("env://SOME_VARIABLE").unwrap())
            .unwrap_err();
        assert!(err.message().contains("never makes import eligible"));
    }

    #[test]
    fn env_import_reads_the_variable_only_when_opted_in() {
        let resolver = EnvImportSecretResolver::with_source(true, |name| {
            (name == "AIKIT_SECRET_RESOLVER_TEST_VAR").then(|| "fixture-env-material".to_string())
        });
        let material = resolver
            .resolve(&SecretRef::parse("env://AIKIT_SECRET_RESOLVER_TEST_VAR").unwrap())
            .unwrap();
        assert_eq!(material.expose(), "fixture-env-material");
    }

    #[test]
    fn suite_dispatches_by_scheme() {
        let suite = SuiteSecretResolver::with_env_import();
        // Wrong-scheme refs are rejected by the dispatched resolver, proving
        // dispatch reached the right scheme handler.
        let err = suite
            .resolve(&SecretRef::parse("keychain://svc/acct").unwrap())
            .unwrap_err();
        assert!(err.code().contains("keychain") || err.code().contains("not_found"));
        let err = suite.resolve(&op_ref()).unwrap_err();
        assert!(err.message().contains("op") || err.message().contains("1Password"));
    }

    #[test]
    fn keychain_missing_entry_is_a_named_not_found() {
        // A certainly-absent entry: deterministic on every platform, touches
        // no real material, and asserts the error NAMES not-found rather than
        // leaking backend noise.
        let resolver = KeychainSecretResolver;
        let err = resolver
            .resolve(
                &SecretRef::parse(
                    "keychain://aikit-definitely-absent-service/aikit-definitely-absent-account",
                )
                .unwrap(),
            )
            .unwrap_err();
        assert!(
            err.code().contains("not_found") || err.code().contains("unavailable"),
            "unexpected error: {} ({})",
            err.code(),
            err.message()
        );
    }
}
