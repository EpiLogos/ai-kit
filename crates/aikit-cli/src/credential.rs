//! Credential application flow shared by CLI and the small initial-config panel.
//!
//! Three ways a credential comes to exist, all owner-native:
//! the OS secure store (`aikit credential setup` prompting once for the
//! material), an explicit environment import (`--from-env --env-var`), and a
//! declared reference (`--ref op://…` / `varlock://…` / `pass://…`) that names
//! where an external store already holds the material — AIKit stores the
//! location, and a resolver materialises from it at the one moment of use.
//! Lifecycle metadata (first-bound and last-rotated timestamps) is stamped by
//! this flow, never inferred; `rotate` and `revoke` close the lifecycle; and
//! `discover` surfaces candidate keys already on the machine with
//! presence-only findings — a discovery finding carries a variable name and a
//! location, never a value.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use aikit_adapters::credential_verify::{
    check_provider_key, known_check, CredentialCheckOutcome, CredentialVerdict,
};
use aikit_adapters::runner::CommandRunner;
use aikit_adapters::secret_resolver::SuiteSecretResolver;
use aikit_adapters::{
    EnvironmentImportProvider, NativeSecureStoreProvider, NativeSecureStoreStatus,
};
use aikit_core::credential::{
    resolve_registered_credential, CredentialBindingState, CredentialProviderRejection,
    CredentialRef, CredentialResolution, SecretMaterialisationClass, SecretProvider,
    SecretProviderDescriptor, SecretProviderRef, SecretProviderTier, SecretRequirement,
    SecretRequirementRef, SecretValue,
};
use aikit_core::secret_ref::{SecretRef, SecretResolver as _};
use aikit_core::{AikitError, Result};
use aikit_store::{AikitHome, CredentialBindingStore};
use aikit_tui::{render_credential_setup_panel, CredentialSetupView};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

#[derive(Clone, Debug)]
pub struct CredentialRequest {
    pub credential: CredentialRef,
    pub consumer_ref: String,
    pub purpose: String,
    pub env_var: Option<String>,
    pub project_env: Option<PathBuf>,
    pub from_env: bool,
    pub headless: bool,
    /// Declare the material's location instead of binding material. The ref
    /// is stored as-is; a resolver materialises from it at use time.
    pub declared_ref: Option<SecretRef>,
    /// Read the material from standard input (one line) instead of a
    /// terminal prompt — the path for a caller that hands the key over a
    /// pipe. Never implied; refused when stdin is a terminal.
    pub stdin: bool,
}

pub fn now_unix_seconds() -> u64 {
    jiff::Timestamp::now().as_second().max(0) as u64
}

#[derive(Debug)]
pub struct CredentialInspection {
    pub resolution: CredentialResolution,
    pub persisted_binding: Option<CredentialBindingState>,
    pub native_provider: SecretProviderDescriptor,
    pub env_available: bool,
}

#[derive(Debug)]
pub struct CredentialSetupOutcome {
    pub resolution: CredentialResolution,
    pub binding: CredentialBindingState,
    pub newly_bound: bool,
}

fn requirement(request: &CredentialRequest) -> Result<SecretRequirement> {
    SecretRequirementRef::new(format!(
        "secret-requirement:{}",
        request.credential.as_str()
    ))
    .map(|requirement_ref| SecretRequirement {
        requirement_ref,
        credential_ref: request.credential.clone(),
        consumer_ref: request.consumer_ref.clone(),
        purpose: request.purpose.clone(),
        permitted_materialisation: [
            SecretMaterialisationClass::ProviderNativeLease,
            SecretMaterialisationClass::ProcessEnv,
        ]
        .into_iter()
        .collect(),
    })
}

pub fn inspect(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialInspection> {
    let persisted_binding = CredentialBindingStore::new(home).load(&request.credential)?;
    let native = NativeSecureStoreProvider::with_binding(persisted_binding.as_ref());
    let native_descriptor = native.descriptor(&request.credential);
    let env = request
        .env_var
        .as_ref()
        .map(|env_var| {
            if request.from_env {
                EnvironmentImportProvider::from_process(
                    request.credential.clone(),
                    env_var.clone(),
                    request.project_env.as_deref(),
                )
            } else {
                EnvironmentImportProvider::discover(
                    request.credential.clone(),
                    env_var.clone(),
                    request.project_env.as_deref(),
                )
            }
        })
        .transpose()?;

    let mut providers: Vec<&dyn SecretProvider> = vec![&native];
    if let Some(env) = env.as_ref() {
        providers.push(env);
    }
    let resolution = resolve_registered_credential(
        requirement(request)?,
        &providers,
        request.headless,
        request.from_env,
    )?;
    let env_available = env
        .as_ref()
        .map(|provider| {
            provider
                .descriptor(&request.credential)
                .supported_credentials
                .contains(&request.credential)
        })
        .unwrap_or(false);
    let persisted_binding = CredentialBindingStore::new(home).load(&request.credential)?;
    Ok(CredentialInspection {
        resolution,
        persisted_binding,
        native_provider: native_descriptor,
        env_available,
    })
}

pub fn setup(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialSetupOutcome> {
    if let Some(secret_ref) = request.declared_ref.clone() {
        return declare_ref(home, request, secret_ref);
    }
    if request.stdin {
        let secret = read_piped_secret(
            io::stdin().lock(),
            io::IsTerminal::is_terminal(&io::stdin()),
        )?;
        return bind_native_material(home, request, secret);
    }
    let inspection = inspect(home, request)?;
    if inspection.resolution.selected() {
        return selected_outcome(home, request, inspection.resolution, false);
    }
    if request.headless {
        return Err(unresolved_headless(&inspection.resolution));
    }

    let view = CredentialSetupView {
        credential_ref: request.credential.clone(),
        consumer_ref: request.consumer_ref.clone(),
        purpose: request.purpose.clone(),
        native_provider: inspection.native_provider.clone(),
        env_var: request.env_var.clone(),
        env_available: inspection.env_available,
        encrypted_fallback_available: cfg!(target_os = "linux")
            && !inspection.native_provider.available,
        headless: false,
    };
    eprintln!(
        "{}",
        render_credential_setup_panel(&view, aikit_tui::layout::Glyphs::from_env())
    );
    eprint!("> ");
    io::stderr().flush().map_err(io_error)?;
    let choice = read_line()?.trim().to_ascii_lowercase();
    match choice.as_str() {
        "1" => bind_native(home, request),
        "2" => explicit_env(home, request),
        "3" => bind_encrypted_fallback(home, request),
        "q" | "quit" | "cancel" => Err(AikitError::new(
            "credential.setup_cancelled",
            "credential setup was cancelled",
        )),
        _ => Err(AikitError::new(
            "credential.setup_invalid_choice",
            "choose 1, 2, 3, or q",
        )),
    }
}

/// The binding record a declared reference produces. The material stays in
/// the external store; this record names the resolver route to it. Declared
/// refs are brokered facts, not keychain items, so `NativeSecureStoreProvider`
/// never mistakes them for its own bound set.
fn declared_ref_binding(
    credential: &CredentialRef,
    secret_ref: SecretRef,
) -> Result<CredentialBindingState> {
    if matches!(secret_ref, SecretRef::Env { .. }) {
        return Err(AikitError::new(
            "credential.declared_env_ref_refused",
            "env:// names a transient process variable, not a store; import explicitly \
             with --from-env --env-var instead",
        ));
    }
    let scheme = secret_ref.scheme().to_string();
    let mut metadata = BTreeMap::new();
    metadata.insert("scheme".into(), scheme.clone());
    Ok(CredentialBindingState {
        credential_ref: credential.clone(),
        provider_ref: SecretProviderRef::new(format!("provider:secret-resolver/{scheme}"))?,
        provider_tier: SecretProviderTier::BrokeredSecureProvider,
        materialisation: SecretMaterialisationClass::CredentialBroker,
        binding_provenance: secret_ref.to_string(),
        revision_or_lease_class: Some("declared-secret-ref/v1".into()),
        expires_at: None,
        revoked: false,
        metadata,
        declared_secret_ref: Some(secret_ref),
        bound_at_unix_seconds: None,
        last_rotated_at_unix_seconds: None,
        last_verified_at_unix_seconds: None,
    })
}

/// `aikit credential setup <ref> --ref …`: record where the material lives.
/// No material is read, prompted for or stored — declaring a location is the
/// one binding act that never touches a secret at all.
fn declare_ref(
    home: &AikitHome,
    request: &CredentialRequest,
    secret_ref: SecretRef,
) -> Result<CredentialSetupOutcome> {
    let store = CredentialBindingStore::new(home);
    let previous = store.load(&request.credential)?;
    if let Some(existing) = &previous {
        if !existing.revoked
            && existing.provider_tier == SecretProviderTier::BrokeredSecureProvider
            && existing.declared_secret_ref.as_ref() == Some(&secret_ref)
        {
            return Ok(CredentialSetupOutcome {
                resolution: resolve_registered_credential(
                    requirement(request)?,
                    &[&NativeSecureStoreProvider::new()],
                    true,
                    false,
                )?,
                binding: existing.clone(),
                newly_bound: false,
            });
        }
    }
    let binding = declared_ref_binding(&request.credential, secret_ref)?.with_lifecycle(
        previous.as_ref(),
        previous.is_some(),
        now_unix_seconds(),
    );
    store.save(&binding)?;
    let resolution = resolve_registered_credential(
        requirement(request)?,
        &[&NativeSecureStoreProvider::new()],
        true,
        false,
    )?;
    Ok(CredentialSetupOutcome {
        resolution,
        binding,
        newly_bound: true,
    })
}

#[derive(Debug)]
pub struct CredentialRotationOutcome {
    pub binding: CredentialBindingState,
    pub notes: Vec<String>,
}

/// `aikit credential rotate`: replace the material or its location while the
/// credential ref stays stable. Either a new declared ref (`--ref`) or fresh
/// material imported explicitly from the environment (`--from-env --env-var`,
/// stored into the OS secure store) is required; rotation never prompts.
pub fn rotate(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialRotationOutcome> {
    let store = CredentialBindingStore::new(home);
    let previous = store.load(&request.credential)?;
    let mut notes = Vec::new();
    let binding = if let Some(secret_ref) = request.declared_ref.clone() {
        if previous
            .as_ref()
            .is_some_and(|previous| previous.provider_tier == SecretProviderTier::OsSecureStore)
        {
            notes.push(
                "the previous binding pointed at the OS secure store; its keychain item, \
                 if any, was left in place"
                    .into(),
            );
        }
        declared_ref_binding(&request.credential, secret_ref)?.with_lifecycle(
            previous.as_ref(),
            true,
            now_unix_seconds(),
        )
    } else if request.stdin {
        let secret = read_piped_secret(
            io::stdin().lock(),
            io::IsTerminal::is_terminal(&io::stdin()),
        )?;
        let native = NativeSecureStoreProvider::new();
        if native.status(&request.credential) == NativeSecureStoreStatus::Unavailable {
            return Err(AikitError::new(
                "credential.native_store_unavailable",
                "the OS secure store is unavailable; rotate by declaring a ref (--ref) instead",
            ));
        }
        let mut fresh = native.bind(&request.credential, &secret)?;
        fresh.declared_secret_ref = None;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.declared_secret_ref.is_some())
        {
            notes.push(
                "the previous binding declared an external store location; it has been \
                 replaced by the OS secure store binding"
                    .into(),
            );
        }
        fresh.with_lifecycle(previous.as_ref(), true, now_unix_seconds())
    } else if request.from_env {
        let env_var = request.env_var.as_ref().ok_or_else(|| {
            AikitError::new(
                "credential.env_var_required",
                "rotating from imported material requires --env-var NAME with --from-env",
            )
        })?;
        let environment = EnvironmentImportProvider::from_process(
            request.credential.clone(),
            env_var.clone(),
            request.project_env.as_deref(),
        )?;
        let secret = environment
            .materialise(&request.credential, SecretMaterialisationClass::ProcessEnv)?
            .ok_or_else(|| {
                AikitError::new(
                    "credential.env_missing",
                    format!("{env_var} is not present in the selected shell/project environment"),
                )
            })?;
        let native = NativeSecureStoreProvider::new();
        if native.status(&request.credential) == NativeSecureStoreStatus::Unavailable {
            return Err(AikitError::new(
                "credential.native_store_unavailable",
                "the OS secure store is unavailable; rotate by declaring a ref (--ref) instead",
            ));
        }
        let mut fresh = native.bind(&request.credential, &secret)?;
        // The material now lives in the native store; a stale declared
        // location would resolve to the old material and must not survive.
        fresh.declared_secret_ref = None;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.declared_secret_ref.is_some())
        {
            notes.push(
                "the previous binding declared an external store location; it has been \
                 replaced by the OS secure store binding"
                    .into(),
            );
        }
        fresh.with_lifecycle(previous.as_ref(), true, now_unix_seconds())
    } else {
        return Err(AikitError::new(
            "credential.rotation_source_required",
            "rotation needs a source: --ref SECRET_REF to declare a new location, or \
             --from-env --env-var NAME to import fresh material into the OS secure store",
        ));
    };
    store.save(&binding)?;
    Ok(CredentialRotationOutcome { binding, notes })
}

/// `aikit credential revoke`: mark the binding revoked so resolution and
/// dispatch refuse it at the next use. Nothing operator-owned is deleted —
/// the keychain item or vault entry stays exactly where it is.
pub fn revoke(home: &AikitHome, credential: &CredentialRef) -> Result<CredentialBindingState> {
    let store = CredentialBindingStore::new(home);
    let mut binding = store.load(credential)?.ok_or_else(|| {
        AikitError::new(
            "credential.binding_missing",
            format!(
                "no persisted binding exists for {}; nothing to revoke",
                credential.as_str()
            ),
        )
    })?;
    binding.revoked = true;
    store.save(&binding)?;
    Ok(binding)
}

// ---------------------------------------------------------------------------
// Verify: one operator-invoked live key check
// ---------------------------------------------------------------------------

/// The answer of one `aikit credential verify` run: a verdict, the HTTP
/// status class that produced it, and whether the check was definitive
/// enough to record into the binding's lifecycle. No field carries material —
/// the key and the Authorization header are never part of an outcome.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct CredentialVerifyOutcome {
    pub credential: String,
    pub provider: String,
    pub verdict: CredentialVerdict,
    pub http_status_class: Option<String>,
    /// Whether the outcome definitively establishes key quality (worked, or
    /// a 401/403 refusal). Only definitive outcomes are recorded.
    pub definitive: bool,
    pub checked_at_unix_seconds: u64,
    pub recorded: bool,
    pub notes: Vec<String>,
}

/// `aikit credential verify <CREDENTIAL>`: materialise the bound credential
/// through the same seam every consumer uses and run ONE minimal live check
/// against its provider. Operator-invoked only — no launch, resolution or
/// detection path ever calls this, because a live check spends the key
/// against the provider and belongs to the operator alone. A provider with
/// no known check is refused honestly (the known-check refusal comes before
/// any materialisation, so an unverifiable credential never touches a
/// store); an inconclusive check — unreachable, rate-limited — records
/// nothing.
pub fn verify(
    home: &AikitHome,
    credential: &CredentialRef,
    runner: &dyn CommandRunner,
) -> Result<CredentialVerifyOutcome> {
    let store = CredentialBindingStore::new(home);
    let binding = store.load(credential)?.ok_or_else(|| {
        AikitError::new(
            "credential.verify_unbound",
            format!(
                "no binding exists for {credential_ref}; bind it with `aikit credential setup` \
                 first — verify answers for bound credentials only",
                credential_ref = credential.as_str()
            ),
        )
    })?;
    if binding.revoked {
        return Err(AikitError::new(
            "credential.verify_revoked",
            "the binding is revoked; rotate or re-bind before verifying — verify does \
             not bypass revocation",
        ));
    }
    let provider = credential
        .as_str()
        .strip_prefix("credential:")
        .unwrap_or(credential.as_str());
    known_check(provider).ok_or_else(|| {
        AikitError::new(
            "credential.verify_provider_unknown",
            format!(
                "no live check is known for provider {provider:?}; verify refuses to fake \
                 a pass (known providers: {})",
                aikit_adapters::credential_verify::KNOWN_CHECKS
                    .iter()
                    .map(|check| check.provider)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )
    })?;
    let material = materialise_bound(&binding)?;
    let outcome = check_provider_key(runner, provider, &material)?;
    let checked_at_unix_seconds = now_unix_seconds();
    let updated = stamp_verification(&binding, &outcome, checked_at_unix_seconds);
    let recorded = updated.is_some();
    if let Some(updated) = updated {
        store.save(&updated)?;
    }
    let notes = match (outcome.verdict, outcome.definitive) {
        (CredentialVerdict::Working, _) => vec![
            "the key works right now; the check time was recorded against the binding".to_string(),
        ],
        (CredentialVerdict::Refused, true) => vec![format!(
            "the provider definitively refused this key (HTTP {}); the refusal was \
             recorded — rotate or re-bind before relying on it",
            outcome.http_status_class.as_deref().unwrap_or("?")
        )],
        (CredentialVerdict::Refused, false) => vec![format!(
            "the provider refused the request (HTTP {}) without proving the key bad, \
             for example a rate limit; nothing was recorded",
            outcome.http_status_class.as_deref().unwrap_or("?")
        )],
        (CredentialVerdict::Unreachable, _) => {
            vec!["the check could not reach a verdict; nothing was recorded".to_string()]
        }
    };
    Ok(CredentialVerifyOutcome {
        credential: credential.as_str().to_string(),
        provider: provider.to_string(),
        verdict: outcome.verdict,
        http_status_class: outcome.http_status_class,
        definitive: outcome.definitive,
        checked_at_unix_seconds,
        recorded,
        notes,
    })
}

/// Materialise the bound credential through the same seam every consumer
/// uses: a declared ref resolves straight from the external store the
/// operator named; otherwise the OS secure store answers for its own binding.
fn materialise_bound(binding: &CredentialBindingState) -> Result<SecretValue> {
    if let Some(secret_ref) = &binding.declared_secret_ref {
        return SuiteSecretResolver::default().resolve(secret_ref);
    }
    NativeSecureStoreProvider::with_binding(Some(binding))
        .materialise(
            &binding.credential_ref,
            SecretMaterialisationClass::ProcessEnv,
        )?
        .ok_or_else(|| {
            AikitError::new(
                "credential.verify_no_material",
                format!(
                    "the bound provider holds no material for {}; rotate or re-bind it",
                    binding.credential_ref.as_str()
                ),
            )
        })
}

/// The lifecycle stamp rule, kept pure for tests: a definitive outcome
/// records the check time; an inconclusive one leaves the record untouched.
fn stamp_verification(
    binding: &CredentialBindingState,
    outcome: &CredentialCheckOutcome,
    checked_at_unix_seconds: u64,
) -> Option<CredentialBindingState> {
    if !outcome.definitive {
        return None;
    }
    let mut updated = binding.clone();
    updated.last_verified_at_unix_seconds = Some(checked_at_unix_seconds);
    Some(updated)
}

// ---------------------------------------------------------------------------
// Discovery: candidate keys already on this machine, presence only
// ---------------------------------------------------------------------------

/// One candidate key found on the machine. A finding carries a variable name
/// and a location — never a value; there is no field that could.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CredentialDiscoveryFinding {
    /// `shell-env`, `project-env` or `harness-auth`.
    pub source: &'static str,
    /// Where the name was seen (file path or "process environment").
    pub location: String,
    /// The variable or key name, as seen.
    pub name: String,
    /// The credential ref a bind would satisfy, when the vendor is known.
    pub proposed_credential_ref: Option<String>,
    /// The env var an explicit `--from-env` import would read.
    pub proposed_env_var: Option<String>,
    /// Whether a non-revoked binding for the proposed ref already exists.
    pub already_bound: bool,
}

/// Known vendor prefixes and the provider segment of the credential ref each
/// one maps to. Voice and speech providers sit here beside the classic LLM
/// providers: a key is a key per provider, whatever it serves.
const VENDOR_PREFIXES: &[(&str, &str)] = &[
    ("OPENAI", "openai"),
    ("ANTHROPIC", "anthropic"),
    ("GEMINI", "gemini"),
    ("GOOGLE", "google"),
    ("ZAI", "zai"),
    ("ZHIPU", "zhipu"),
    ("DEEPSEEK", "deepseek"),
    ("OPENROUTER", "openrouter"),
    ("MOONSHOT", "moonshot"),
    ("DASHSCOPE", "dashscope"),
    ("GROQ", "groq"),
    ("MISTRAL", "mistral"),
    ("XAI", "xai"),
    ("HUGGINGFACE", "huggingface"),
    ("TOGETHER", "together"),
    ("FIREWORKS", "fireworks"),
    ("PERPLEXITY", "perplexity"),
    ("COHERE", "cohere"),
    ("VOYAGE", "voyage"),
    ("ELEVENLABS", "elevenlabs"),
    ("CARTESIA", "cartesia"),
    ("DEEPGRAM", "deepgram"),
    ("ASSEMBLYAI", "assemblyai"),
];

/// Variable names that can never be provider keys: AIKit's own and the
/// dynamic-linker escapes.
fn is_excluded_variable(name: &str) -> bool {
    ["AIKIT_", "CENTRAL_", "WORKCELL_", "LD_", "DYLD_"]
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

/// Whether a variable name has the shape of a provider credential
/// (`<VENDOR>_API_KEY` / `_TOKEN` / `_KEY` with a real vendor prefix).
fn is_candidate_variable(name: &str) -> bool {
    if is_excluded_variable(name) {
        return false;
    }
    let shaped = name.ends_with("_API_KEY") || name.ends_with("_TOKEN") || name.ends_with("_KEY");
    shaped
        && name
            .strip_suffix("_API_KEY")
            .or_else(|| name.strip_suffix("_TOKEN"))
            .or_else(|| name.strip_suffix("_KEY"))
            .is_some_and(|prefix| prefix.len() >= 2)
}

/// The credential ref a discovered variable name proposes, when the vendor is
/// known. Shape follows the catalogue's synthesised requirements
/// (`credential:<provider>`), so binding the proposal satisfies the route
/// condition the model router actually checks.
fn propose_credential_ref(name: &str) -> Option<String> {
    if is_excluded_variable(name) {
        return None;
    }
    let prefix = name
        .strip_suffix("_API_KEY")
        .or_else(|| name.strip_suffix("_TOKEN"))
        .or_else(|| name.strip_suffix("_KEY"))?;
    if prefix.len() < 2 {
        return None;
    }
    let upper = prefix.to_ascii_uppercase();
    VENDOR_PREFIXES
        .iter()
        .find(|(vendor, _)| *vendor == upper)
        .map(|(_, provider)| format!("credential:{provider}"))
}

/// Variable names in a dotenv-shaped text. Only the key side is kept; values
/// are never parsed into memory.
fn dotenv_names(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line).trim();
        if let Some((key, _)) = line.split_once('=') {
            let key = key.trim();
            if !key.is_empty() {
                names.push(key.to_string());
            }
        }
    }
    names
}

/// Top-level key names of a JSON auth file. Values are discarded by
/// construction: only the object's key set is read.
fn json_key_names(bytes: &[u8]) -> Option<Vec<String>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some(value.as_object()?.keys().cloned().collect::<Vec<String>>())
}

/// Candidate harness auth files, as (harness, path-relative-to-HOME) pairs.
/// The list is deliberately short and named; discovery never sweeps the home
/// directory.
const HARNESS_AUTH_FILES: &[(&str, &str)] = &[
    ("codex", ".codex/auth.json"),
    ("claude", ".claude/.credentials.json"),
    ("pi", ".pi/agent/auth.json"),
];

/// Scan the machine for candidate provider keys: shell environment names, the
/// project's dotenv files and a short, named list of harness auth files.
/// Presence only — findings name what was seen and where, never what the
/// value is.
pub fn discover(
    home: &AikitHome,
    project_root: Option<&Path>,
    extra_env_file: Option<&Path>,
) -> Result<Vec<CredentialDiscoveryFinding>> {
    let bindings = CredentialBindingStore::new(home).list()?;
    let bound_refs: BTreeSet<String> = bindings
        .iter()
        .filter(|binding| !binding.revoked)
        .map(|binding| binding.credential_ref.as_str().to_string())
        .collect();

    let mut findings: BTreeMap<(String, String, String), CredentialDiscoveryFinding> =
        BTreeMap::new();
    let push =
        |source: &'static str,
         location: String,
         name: String,
         findings: &mut BTreeMap<(String, String, String), CredentialDiscoveryFinding>| {
            let proposed = propose_credential_ref(&name);
            let already_bound = proposed
                .as_ref()
                .map(|reference| bound_refs.contains(reference))
                .unwrap_or(false);
            findings.insert(
                (source.to_string(), location.clone(), name.clone()),
                CredentialDiscoveryFinding {
                    source,
                    location,
                    proposed_credential_ref: proposed,
                    proposed_env_var: Some(name.clone()),
                    name,
                    already_bound,
                },
            );
        };

    for name in std::env::vars().map(|(name, _)| name) {
        if is_candidate_variable(&name) {
            push(
                "shell-env",
                "process environment".into(),
                name,
                &mut findings,
            );
        }
    }

    let mut dotenv_paths: Vec<PathBuf> = Vec::new();
    if let Some(root) = project_root {
        dotenv_paths.push(root.join(".env"));
        dotenv_paths.push(root.join(".env.local"));
    }
    if let Some(extra) = extra_env_file {
        dotenv_paths.push(extra.to_path_buf());
    }
    for path in dotenv_paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for name in dotenv_names(&text) {
            push(
                "project-env",
                path.display().to_string(),
                name,
                &mut findings,
            );
        }
    }

    if let Ok(home_dir) = std::env::var("HOME") {
        for (harness, relative) in HARNESS_AUTH_FILES {
            let path = Path::new(&home_dir).join(relative);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Some(names) = json_key_names(&bytes) else {
                continue;
            };
            for name in names {
                push(
                    "harness-auth",
                    format!("{} ({harness})", path.display()),
                    name,
                    &mut findings,
                );
            }
        }
    }

    Ok(findings.into_values().collect())
}

fn selected_outcome(
    home: &AikitHome,
    request: &CredentialRequest,
    resolution: CredentialResolution,
    newly_bound: bool,
) -> Result<CredentialSetupOutcome> {
    let selected = resolution.selected_provider_ref.as_ref().ok_or_else(|| {
        AikitError::new(
            "credential.unresolved",
            "credential resolution selected no provider",
        )
    })?;
    let stored_binding = CredentialBindingStore::new(home).load(&request.credential)?;
    let native = NativeSecureStoreProvider::with_binding(stored_binding.as_ref());
    let binding = if selected.as_str().starts_with("provider:os-secure-store/") {
        native.binding_state(&request.credential)?.ok_or_else(|| {
            AikitError::new(
                "credential.binding_missing",
                "native provider was selected but no safe binding record is available",
            )
        })?
    } else if selected.as_str() == "provider:explicit-environment-import" {
        let env_var = request.env_var.as_ref().ok_or_else(|| {
            AikitError::new(
                "credential.env_var_required",
                "explicit environment import requires --env-var",
            )
        })?;
        let env = EnvironmentImportProvider::from_process(
            request.credential.clone(),
            env_var.clone(),
            request.project_env.as_deref(),
        )?;
        env.binding_state(&request.credential)?.ok_or_else(|| {
            AikitError::new(
                "credential.env_missing",
                format!("{env_var} is not present in the selected shell/project environment"),
            )
        })?
    } else {
        return Err(AikitError::new(
            "credential.provider_unknown",
            format!(
                "selected provider {} is not wired into this setup flow",
                selected.as_str()
            ),
        ));
    };
    // Persist only durable provider bindings. Environment import is deliberately
    // transient and must be explicitly selected again on a later invocation.
    if binding.provider_tier
        != aikit_core::credential::SecretProviderTier::ExplicitEnvironmentImport
    {
        CredentialBindingStore::new(home).save(&binding)?;
    }
    Ok(CredentialSetupOutcome {
        resolution,
        binding,
        newly_bound,
    })
}

fn bind_native(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialSetupOutcome> {
    let native = NativeSecureStoreProvider::new();
    if native.status(&request.credential) == NativeSecureStoreStatus::Unavailable {
        return Err(AikitError::new(
            "credential.native_store_unavailable",
            "the OS secure store is unavailable; choose the explicit environment path or Linux encrypted fallback",
        ));
    }
    let secret = read_secret("Secret: ")?;
    bind_native_material(home, request, secret)
}

/// Bind material already in hand into the OS secure store (the prompt and
/// the `--stdin` pipe share this one path).
fn bind_native_material(
    home: &AikitHome,
    request: &CredentialRequest,
    secret: SecretValue,
) -> Result<CredentialSetupOutcome> {
    let native = NativeSecureStoreProvider::new();
    if native.status(&request.credential) == NativeSecureStoreStatus::Unavailable {
        return Err(AikitError::new(
            "credential.native_store_unavailable",
            "the OS secure store is unavailable; declare where the key lives (--ref) instead",
        ));
    }
    let binding = native.bind(&request.credential, &secret)?;
    CredentialBindingStore::new(home).save(&binding)?;
    let rebound_native = NativeSecureStoreProvider::with_binding(Some(&binding));
    let resolution =
        resolve_registered_credential(requirement(request)?, &[&rebound_native], false, false)?;
    Ok(CredentialSetupOutcome {
        resolution,
        binding,
        newly_bound: true,
    })
}

fn explicit_env(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialSetupOutcome> {
    let env_var = request.env_var.as_ref().ok_or_else(|| {
        AikitError::new(
            "credential.env_var_required",
            "environment import requires a named source; pass --env-var NAME",
        )
    })?;
    let env = EnvironmentImportProvider::from_process(
        request.credential.clone(),
        env_var.clone(),
        request.project_env.as_deref(),
    )?;
    let resolution =
        resolve_registered_credential(requirement(request)?, &[&env], request.headless, true)?;
    if !resolution.selected() {
        return Err(AikitError::new(
            "credential.env_missing",
            format!("{env_var} is not present in the selected shell/project environment"),
        ));
    }
    selected_outcome(home, request, resolution, false)
}

#[cfg(target_os = "linux")]
fn bind_encrypted_fallback(
    home: &AikitHome,
    request: &CredentialRequest,
) -> Result<CredentialSetupOutcome> {
    use aikit_adapters::LinuxEncryptedFallbackProvider;

    let native = NativeSecureStoreProvider::new();
    if native.status(&request.credential) != NativeSecureStoreStatus::Unavailable {
        return Err(AikitError::new(
            "credential.fallback_not_permitted",
            "encrypted fallback is offered only when Secret Service is unavailable",
        ));
    }
    eprintln!(
        "Encrypted fallback stores ciphertext under AIKit state. It is tier explicit-encrypted-fallback and will not be upgraded silently."
    );
    let secret = read_secret("Secret: ")?;
    let passphrase = read_secret("Fallback encryption passphrase: ")?;
    let provider = LinuxEncryptedFallbackProvider::new(
        home.state().join("credential-fallback"),
        passphrase,
        false,
    );
    let binding = provider.bind(&request.credential, &secret)?;
    CredentialBindingStore::new(home).save(&binding)?;
    let resolution =
        resolve_registered_credential(requirement(request)?, &[&provider], false, false)?;
    Ok(CredentialSetupOutcome {
        resolution,
        binding,
        newly_bound: true,
    })
}

#[cfg(not(target_os = "linux"))]
fn bind_encrypted_fallback(
    _home: &AikitHome,
    _request: &CredentialRequest,
) -> Result<CredentialSetupOutcome> {
    Err(AikitError::new(
        "credential.fallback_not_available",
        "the encrypted local fallback is Linux-only",
    ))
}

fn unresolved_headless(resolution: &CredentialResolution) -> AikitError {
    let rejection = resolution
        .provider_explanations
        .iter()
        .filter_map(|explanation| explanation.rejection.as_ref())
        .find(|rejection| **rejection == CredentialProviderRejection::CredentialNotBound)
        .or_else(|| {
            resolution
                .provider_explanations
                .iter()
                .filter_map(|explanation| explanation.rejection.as_ref())
                .next()
        });
    let rejection = rejection.map(rejection_name).unwrap_or("unresolved");
    AikitError::new(
        "credential.unresolved_headless",
        format!("headless credential resolution failed: {rejection}"),
    )
    .with("rejection", rejection.to_string())
}

fn rejection_name(rejection: &CredentialProviderRejection) -> &'static str {
    match rejection {
        CredentialProviderRejection::Unavailable => "unavailable",
        CredentialProviderRejection::NotHeadlessCapable => "not-headless-capable",
        CredentialProviderRejection::CredentialNotBound => "credential-not-bound",
        CredentialProviderRejection::NoPermittedMaterialisation => "no-permitted-materialisation",
        CredentialProviderRejection::EnvironmentImportNotExplicitlyAllowed => {
            "environment-import-not-explicitly-allowed"
        }
    }
}

fn read_line() -> Result<String> {
    let mut line = String::new();
    io::stdin().read_line(&mut line).map_err(io_error)?;
    Ok(line)
}

struct RawModeGuard;

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

fn read_secret(label: &str) -> Result<SecretValue> {
    eprint!("{label}");
    io::stderr().flush().map_err(io_error)?;
    enable_raw_mode().map_err(|error| {
        AikitError::new(
            "credential.prompt_unavailable",
            format!("could not disable terminal echo for secret input: {error}"),
        )
    })?;
    let _guard = RawModeGuard;
    let mut secret = String::new();
    loop {
        match event::read().map_err(|error| {
            AikitError::new(
                "credential.prompt_unavailable",
                format!("could not read secret input: {error}"),
            )
        })? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => break,
                KeyCode::Esc => {
                    return Err(AikitError::new(
                        "credential.setup_cancelled",
                        "credential setup was cancelled",
                    ))
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char(ch) => secret.push(ch),
                _ => {}
            },
            _ => {}
        }
    }
    eprintln!();
    SecretValue::new(secret)
}

/// Read one line of key material from a pipe. A terminal is refused (the
/// prompt without `--stdin` hides typing; a pipe never echoes); an empty
/// line is refused; the trailing newline is not part of the key.
fn read_piped_secret(mut reader: impl io::BufRead, is_terminal: bool) -> Result<SecretValue> {
    if is_terminal {
        return Err(AikitError::new(
            "credential.stdin_is_terminal",
            "--stdin reads a key from a pipe; at a terminal, run setup without --stdin to be prompted",
        ));
    }
    let mut line = String::new();
    reader.read_line(&mut line).map_err(io_error)?;
    let material = line.trim_end_matches(['\n', '\r']).trim().to_string();
    if material.is_empty() {
        return Err(AikitError::new(
            "credential.stdin_empty",
            "no key arrived on standard input; nothing was bound",
        ));
    }
    SecretValue::new(material)
}

fn io_error(error: io::Error) -> AikitError {
    AikitError::new(
        "credential.io_failed",
        format!("credential prompt I/O failed: {error}"),
    )
}

#[cfg(test)]
mod stdin_tests {
    use super::*;

    #[test]
    fn a_piped_key_is_one_trimmed_line_and_never_a_terminal() {
        let secret = read_piped_secret(io::Cursor::new(b"sk-pipe-dummy-1234\n"), false).unwrap();
        assert_eq!(secret.expose(), "sk-pipe-dummy-1234");
        let refused = read_piped_secret(io::Cursor::new(b"sk-x\n"), true).unwrap_err();
        assert_eq!(refused.code(), "credential.stdin_is_terminal");
        let empty = read_piped_secret(io::Cursor::new(b"\n"), false).unwrap_err();
        assert_eq!(empty.code(), "credential.stdin_empty");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::EnvironmentImportProvider;
    use aikit_core::credential::{resolve_registered_credential, SecretProvider};

    #[test]
    fn headless_error_preserves_exact_unbound_reason() {
        let credential = CredentialRef::new("credential:test/headless").unwrap();
        let env =
            EnvironmentImportProvider::from_value(credential.clone(), "AIKIT_MISSING_TOKEN", None)
                .unwrap();
        let request = CredentialRequest {
            credential,
            consumer_ref: "harness:test".into(),
            purpose: "test".into(),
            env_var: Some("AIKIT_MISSING_TOKEN".into()),
            project_env: None,
            from_env: false,
            headless: true,
            declared_ref: None,
            stdin: false,
        };
        let resolution = resolve_registered_credential(
            requirement(&request).unwrap(),
            &[&env as &dyn SecretProvider],
            true,
            false,
        )
        .unwrap();
        let error = unresolved_headless(&resolution);
        assert_eq!(error.code(), "credential.unresolved_headless");
        assert!(error.to_string().contains("credential-not-bound"));
    }

    #[test]
    fn declared_ref_binding_is_brokered_and_refuses_env_scheme() {
        let credential = CredentialRef::new("credential:openai").unwrap();
        let err = declared_ref_binding(
            &credential,
            SecretRef::parse("env://OPENAI_API_KEY").unwrap(),
        )
        .unwrap_err();
        assert_eq!(err.code(), "credential.declared_env_ref_refused");

        let binding = declared_ref_binding(
            &credential,
            SecretRef::parse("op://Vault/openai/key").unwrap(),
        )
        .unwrap();
        assert_eq!(
            binding.provider_ref.as_str(),
            "provider:secret-resolver/onepassword"
        );
        assert_eq!(
            binding.provider_tier,
            SecretProviderTier::BrokeredSecureProvider
        );
        assert_eq!(
            binding.declared_secret_ref.as_ref().unwrap().to_string(),
            "op://Vault/openai/key"
        );
        // The provenance is the ref — a location. No material anywhere.
        assert!(binding.binding_provenance.starts_with("op://"));
    }

    #[test]
    fn discovery_proposals_map_known_vendors_and_skip_non_keys() {
        assert_eq!(
            propose_credential_ref("OPENAI_API_KEY").as_deref(),
            Some("credential:openai")
        );
        assert_eq!(
            propose_credential_ref("ZAI_API_KEY").as_deref(),
            Some("credential:zai")
        );
        assert_eq!(
            propose_credential_ref("ELEVENLABS_API_KEY").as_deref(),
            Some("credential:elevenlabs")
        );
        // Known-shaped but unknown vendor: disclosed, never proposed.
        assert_eq!(propose_credential_ref("SOMETHING_API_KEY"), None);
        // AIKit's own surfaces are never provider keys.
        assert_eq!(propose_credential_ref("AIKIT_GATEWAY_TOKEN"), None);
        assert_eq!(propose_credential_ref("LD_PRELOAD_KEY"), None);
        assert_eq!(propose_credential_ref("X_API_KEY"), None);
        assert_eq!(propose_credential_ref("HOME"), None);
    }

    #[test]
    fn dotenv_discovery_keeps_names_and_never_values() {
        let text =
            "# comment\nexport FIRST_API_KEY=sk-value\nSECOND_TOKEN='other-value'\n\nBROKEN\n=";
        let names = dotenv_names(text);
        assert_eq!(names, vec!["FIRST_API_KEY", "SECOND_TOKEN"]);
        let rendered = format!("{names:?}");
        assert!(!rendered.contains("sk-value"));
        assert!(!rendered.contains("other-value"));
    }

    fn seeded_revoked_binding(home: &AikitHome, credential_ref: &CredentialRef) {
        let provider = EnvironmentImportProvider::from_value(
            credential_ref.clone(),
            "AIKIT_DELIVERY_PROBE_SOURCE",
            Some("fixture-material-not-a-real-key".into()),
        )
        .unwrap();
        let mut state = provider.binding_state(credential_ref).unwrap().unwrap();
        state.revoked = true;
        CredentialBindingStore::new(home).save(&state).unwrap();
    }

    fn verify_binding() -> CredentialBindingState {
        declared_ref_binding(
            &CredentialRef::new("credential:openai").unwrap(),
            SecretRef::parse("op://Vault/openai/key").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_definitive_check_is_recorded_and_an_inconclusive_one_is_not() {
        let binding = verify_binding();
        let working = CredentialCheckOutcome {
            provider: "openai".into(),
            verdict: aikit_adapters::credential_verify::CredentialVerdict::Working,
            http_status_class: Some("2xx".into()),
            definitive: true,
        };
        let stamped = stamp_verification(&binding, &working, 1_770_000_000).unwrap();
        assert_eq!(stamped.last_verified_at_unix_seconds, Some(1_770_000_000));
        // A definitive refusal (401/403) is also recorded: the check ran and
        // the key is known bad.
        let refused = CredentialCheckOutcome {
            verdict: aikit_adapters::credential_verify::CredentialVerdict::Refused,
            definitive: true,
            http_status_class: Some("4xx".into()),
            ..working
        };
        assert!(
            stamp_verification(&binding, &refused, 1_770_000_100).is_some(),
            "a definitive refusal must be recorded"
        );
        // An inconclusive outcome records nothing — the key is neither
        // proven good nor bad.
        let rate_limited = CredentialCheckOutcome {
            definitive: false,
            ..refused
        };
        assert!(stamp_verification(&binding, &rate_limited, 1_770_000_200).is_none());
        let unreachable = CredentialCheckOutcome {
            provider: "openai".into(),
            verdict: aikit_adapters::credential_verify::CredentialVerdict::Unreachable,
            definitive: false,
            http_status_class: None,
        };
        assert!(stamp_verification(&binding, &unreachable, 1_770_000_300).is_none());
        assert_eq!(binding.last_verified_at_unix_seconds, None);
    }

    #[test]
    fn verify_refuses_an_unbound_credential_before_any_check() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let runner = aikit_adapters::runner::ScriptedRunner::new();
        let error = verify(
            &home,
            &CredentialRef::new("credential:openai").unwrap(),
            &runner,
        )
        .unwrap_err();
        assert_eq!(error.code(), "credential.verify_unbound");
        assert!(runner.calls().is_empty(), "no network call may be spent");
    }

    #[test]
    fn verify_refuses_a_revoked_credential_without_materialising() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        seeded_revoked_binding(&home, &CredentialRef::new("credential:openai").unwrap());
        let error = verify(
            &home,
            &CredentialRef::new("credential:openai").unwrap(),
            &aikit_adapters::runner::ScriptedRunner::new(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "credential.verify_revoked");
    }

    #[test]
    fn verify_refuses_an_unknown_provider_before_touching_any_store() {
        let temp = tempfile::tempdir().unwrap();
        let home = aikit_store::AikitHome::at(temp.path().join("aikit"));
        let credential_ref = CredentialRef::new("credential:unheardof-vendor").unwrap();
        let provider = EnvironmentImportProvider::from_value(
            credential_ref.clone(),
            "AIKIT_DELIVERY_PROBE_SOURCE",
            Some("fixture-material-not-a-real-key".into()),
        )
        .unwrap();
        let state = provider.binding_state(&credential_ref).unwrap().unwrap();
        CredentialBindingStore::new(&home).save(&state).unwrap();
        let error = verify(
            &home,
            &credential_ref,
            &aikit_adapters::runner::ScriptedRunner::new(),
        )
        .unwrap_err();
        assert_eq!(error.code(), "credential.verify_provider_unknown");
        assert!(
            error.message().contains("unheardof-vendor"),
            "the refusal names the provider: {error}"
        );
    }

    #[test]
    fn json_auth_discovery_reads_key_names_only() {
        let bytes = br#"{"zai.key": "material-that-must-not-leak", "expires": "never"}"#;
        let names = json_key_names(bytes).unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"zai.key".to_string()));
        let rendered = format!("{names:?}");
        assert!(!rendered.contains("material-that-must-not-leak"));
        assert_eq!(json_key_names(b"not json"), None);
    }

    #[test]
    fn candidate_variable_shape_requires_a_vendor_prefix() {
        assert!(is_candidate_variable("OPENAI_API_KEY"));
        assert!(is_candidate_variable("MY_SERVICE_TOKEN"));
        assert!(!is_candidate_variable("X_API_KEY"));
        assert!(!is_candidate_variable("PATH"));
        assert!(!is_candidate_variable("AIKIT_GATEWAY_TOKEN"));
    }
}
