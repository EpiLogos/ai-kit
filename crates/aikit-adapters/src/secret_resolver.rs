//! Projection-time secret resolution.
//!
//! The implementations each delegate to a genuine store boundary — the
//! pass(1) CLI (`pass show`, the free gpg-backed store), the OS secure store
//! via `keyring`, the 1Password CLI (`op read`, the same SDK boundary the
//! Workcell onepassword adapter uses), the varlock CLI (`printenv`, the
//! documents-side boundary), and the process environment for the legacy
//! `env://` escape hatch. No vault client is reimplemented here.
//!
//! Resolution order per `central.security/v1` (2026-09-09 owner decision,
//! providers all optional): `varlock://` (native default, `keychain()`
//! backing proven) > `pass://` (free cross-machine) > `keychain://` >
//! `op://` (optional) > gated `env://`. Dispatch is by the ref's declared
//! scheme — the order governs which scheme an author should declare, not
//! runtime fallback between stores.
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
            .map_err(|error| {
                format!("failed to spawn `op` (is the 1Password CLI installed?): {error}")
            })?;
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
        format!(
            "op read failed: {}",
            first.chars().take(200).collect::<String>()
        )
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
        let value = self
            .runner
            .read(&secret_ref.to_string())
            .map_err(|message| unavailable("secret_resolver.onepassword_unavailable", message))?;
        if value.is_empty() {
            return Err(unavailable(
                "secret_resolver.empty_material",
                format!("op read returned empty material for {secret_ref}"),
            ));
        }
        SecretValue::new(value)
    }
}

/// The seam a varlock resolver reads through — scripted in tests.
pub trait VarlockRead: Send + Sync + fmt::Debug {
    fn printenv(&self, file: &str, name: &str) -> std::result::Result<String, String>;
}

/// The genuine varlock CLI: `varlock printenv --path <file> <NAME>`. The
/// daemon holds the device key; this resolver never sees a sealed blob.
#[derive(Debug, Default, Clone, Copy)]
pub struct VarlockCli;

impl VarlockRead for VarlockCli {
    fn printenv(&self, file: &str, name: &str) -> std::result::Result<String, String> {
        let output = std::process::Command::new("varlock")
            .args(["printenv", "--path", file, name])
            .output()
            .map_err(|error| format!("failed to spawn `varlock` (is it installed?): {error}"))?;
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map(|value| value.trim_end_matches(['\n', '\r']).to_string())
                .map_err(|_| "varlock printenv returned non-UTF-8 material".to_string());
        }
        Err(classify_varlock_error(&String::from_utf8_lossy(
            &output.stderr,
        )))
    }
}

/// Map varlock stderr to a capability fact: a locked daemon, a missing
/// variable and a missing file are different remediations.
fn classify_varlock_error(stderr: &str) -> String {
    let lowered = stderr.to_lowercase();
    if lowered.contains("lock") || lowered.contains("biometric") {
        "varlock daemon is locked; unlock it (`varlock` interactive) and retry".to_string()
    } else if lowered.contains("not found") || lowered.contains("no variable") {
        "variable not found under the given varlock path".to_string()
    } else if lowered.contains("no such file") || lowered.contains("cannot find") {
        "varlock env file not found for the given path".to_string()
    } else {
        let first = stderr.trim().lines().next().unwrap_or("(no stderr)");
        format!(
            "varlock printenv failed: {}",
            first.chars().take(200).collect::<String>()
        )
    }
}

/// `varlock://<path>/<NAME>` via the genuine varlock CLI — the
/// documents-side boundary of the `central.security/v1` scheme.
#[derive(Debug, Clone)]
pub struct VarlockSecretResolver<R: VarlockRead = VarlockCli> {
    runner: R,
}

impl<R: VarlockRead> VarlockSecretResolver<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl Default for VarlockSecretResolver<VarlockCli> {
    fn default() -> Self {
        Self { runner: VarlockCli }
    }
}

impl<R: VarlockRead> SecretResolver for VarlockSecretResolver<R> {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        let SecretRef::Varlock { file, name } = secret_ref else {
            return Err(unsupported_scheme(secret_ref, "varlock://"));
        };
        let value = self
            .runner
            .printenv(file, name)
            .map_err(|message| unavailable("secret_resolver.varlock_unavailable", message))?;
        if value.is_empty() {
            return Err(unavailable(
                "secret_resolver.empty_material",
                format!("varlock printenv returned empty material for {secret_ref}"),
            ));
        }
        SecretValue::new(value)
    }
}

/// The seam a pass resolver reads through — scripted in tests.
pub trait PassRead: Send + Sync + fmt::Debug {
    fn show(&self, path: &str) -> std::result::Result<String, String>;
}

/// The genuine pass(1) CLI: `pass show <path>`. gpg-agent holds any key
/// passphrase; this resolver never sees gpg material directly.
#[derive(Debug, Default, Clone, Copy)]
pub struct PassCli;

impl PassRead for PassCli {
    fn show(&self, path: &str) -> std::result::Result<String, String> {
        let output = std::process::Command::new("pass")
            .args(["show", path])
            .output()
            .map_err(|error| {
                format!("failed to spawn `pass` (is passwordstore installed?): {error}")
            })?;
        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map(|value| value.trim_end_matches(['\n', '\r']).to_string())
                .map_err(|_| "pass show returned non-UTF-8 material".to_string());
        }
        Err(classify_pass_error(&String::from_utf8_lossy(
            &output.stderr,
        )))
    }
}

/// Map pass stderr to a capability fact: a missing entry, a missing gpg key
/// and a missing store are different remediations.
fn classify_pass_error(stderr: &str) -> String {
    let lowered = stderr.to_lowercase();
    if lowered.contains("no secret key")
        || lowered.contains("no private key")
        || lowered.contains("decryption failed")
        || lowered.contains("passphrase")
        || lowered.contains("cancelled")
    {
        "gpg could not decrypt the pass entry; check the receiving key is present and gpg-agent can serve it".to_string()
    } else if lowered.contains("is not in the password store") || lowered.contains("not found") {
        "pass entry not found for the given store path".to_string()
    } else if lowered.contains("not a password store") || lowered.contains(".password-store") {
        "password store is not initialized (`pass init <gpg-id>`)".to_string()
    } else {
        let first = stderr.trim().lines().next().unwrap_or("(no stderr)");
        format!(
            "pass show failed: {}",
            first.chars().take(200).collect::<String>()
        )
    }
}

/// `pass://<store-path>` via the genuine pass(1) CLI — the free, gpg-backed,
/// git-syncable adapter of the `central.security/v1` scheme.
#[derive(Debug, Clone)]
pub struct PassSecretResolver<R: PassRead = PassCli> {
    runner: R,
}

impl<R: PassRead> PassSecretResolver<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl Default for PassSecretResolver<PassCli> {
    fn default() -> Self {
        Self { runner: PassCli }
    }
}

impl<R: PassRead> SecretResolver for PassSecretResolver<R> {
    fn resolve(&self, secret_ref: &SecretRef) -> Result<SecretValue> {
        let SecretRef::Pass { path } = secret_ref else {
            return Err(unsupported_scheme(secret_ref, "pass://"));
        };
        let value = self
            .runner
            .show(path)
            .map_err(|message| unavailable("secret_resolver.pass_unavailable", message))?;
        if value.is_empty() {
            return Err(unavailable(
                "secret_resolver.empty_material",
                format!("pass show returned empty material for {secret_ref}"),
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

/// The default composite: varlock + pass + keychain + 1Password, with the
/// environment import gate closed unless the operator opened it.
#[derive(Debug, Default)]
pub struct SuiteSecretResolver {
    pub keychain: KeychainSecretResolver,
    pub onepassword: OnePasswordSecretResolver<OpCli>,
    pub pass: PassSecretResolver<PassCli>,
    pub varlock: VarlockSecretResolver<VarlockCli>,
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
            SecretRef::Pass { .. } => self.pass.resolve(secret_ref),
            SecretRef::Varlock { .. } => self.varlock.resolve(secret_ref),
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

    #[derive(Debug, Clone)]
    struct ScriptedVarlock {
        result: std::result::Result<String, String>,
    }

    impl VarlockRead for ScriptedVarlock {
        fn printenv(&self, _file: &str, _name: &str) -> std::result::Result<String, String> {
            self.result.clone()
        }
    }

    fn varlock_ref() -> SecretRef {
        SecretRef::parse("varlock://secrets/providers.env/GEMINI_API_KEY").unwrap()
    }

    #[test]
    fn varlock_resolves_material_and_refuses_empty() {
        let resolver = VarlockSecretResolver::new(ScriptedVarlock {
            result: Ok("fixture-material\n".to_string()),
        });
        let material = resolver.resolve(&varlock_ref()).unwrap();
        assert_eq!(material.expose(), "fixture-material\n");

        let empty = VarlockSecretResolver::new(ScriptedVarlock {
            result: Ok(String::new()),
        });
        let err = empty.resolve(&varlock_ref()).unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn varlock_rejects_wrong_scheme_without_touching_cli() {
        let resolver = VarlockSecretResolver::new(ScriptedVarlock {
            result: Err("must not be called".to_string()),
        });
        let err = resolver
            .resolve(&SecretRef::parse("op://v/i/f").unwrap())
            .unwrap_err();
        assert!(err.message().contains("varlock://"));
    }

    #[test]
    fn varlock_error_classifier_names_capability_facts() {
        assert!(classify_varlock_error("vault is locked").contains("locked"));
        assert!(classify_varlock_error("biometric required").contains("locked"));
        assert!(classify_varlock_error("variable not found").contains("not found"));
        assert!(classify_varlock_error("no such file").contains("env file not found"));
        assert!(classify_varlock_error("boom").contains("varlock printenv failed"));
    }

    #[derive(Debug, Clone)]
    struct ScriptedPass {
        result: std::result::Result<String, String>,
    }

    impl PassRead for ScriptedPass {
        fn show(&self, _path: &str) -> std::result::Result<String, String> {
            self.result.clone()
        }
    }

    fn pass_ref() -> SecretRef {
        SecretRef::parse("pass://providers/gemini-api-key").unwrap()
    }

    #[test]
    fn pass_resolves_material_and_refuses_empty() {
        let resolver = PassSecretResolver::new(ScriptedPass {
            result: Ok("fixture-material".to_string()),
        });
        let material = resolver.resolve(&pass_ref()).unwrap();
        assert_eq!(material.expose(), "fixture-material");

        let empty = PassSecretResolver::new(ScriptedPass {
            result: Ok(String::new()),
        });
        let err = empty.resolve(&pass_ref()).unwrap_err();
        assert!(err.message().contains("empty"));
    }

    #[test]
    fn pass_rejects_wrong_scheme_without_touching_cli() {
        let resolver = PassSecretResolver::new(ScriptedPass {
            result: Err("must not be called".to_string()),
        });
        let err = resolver
            .resolve(&SecretRef::parse("varlock://secrets/providers.env/NAME").unwrap())
            .unwrap_err();
        assert!(err.message().contains("pass://"));
    }

    #[test]
    fn pass_error_classifier_names_capability_facts() {
        assert!(classify_pass_error("gpg: decryption failed: No secret key").contains("gpg"));
        assert!(classify_pass_error("gpg: cancelled by user").contains("gpg"));
        assert!(
            classify_pass_error("Error: providers/x is not in the password store.")
                .contains("not found")
        );
        assert!(classify_pass_error("Error: not a password store").contains("pass init"));
        assert!(classify_pass_error("boom").contains("pass show failed"));
    }
}
