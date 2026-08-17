//! Credential application flow shared by CLI and the small initial-config panel.

use std::io::{self, Write};
use std::path::PathBuf;

use aikit_adapters::{EnvironmentImportProvider, NativeSecureStoreProvider, NativeSecureStoreStatus};
use aikit_core::credential::{
    resolve_registered_credential, CredentialBindingState, CredentialProviderRejection,
    CredentialRef, CredentialResolution, SecretMaterialisationClass, SecretProvider,
    SecretProviderDescriptor, SecretRequirement, SecretRequirementRef, SecretValue,
};
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
    let native = NativeSecureStoreProvider::new();
    let native_descriptor = native.descriptor(&request.credential);
    let env = request
        .env_var
        .as_ref()
        .map(|env_var| {
            EnvironmentImportProvider::from_process(
                request.credential.clone(),
                env_var.clone(),
                request.project_env.as_deref(),
            )
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
        .and_then(|provider| provider.binding_state(&request.credential).transpose())
        .transpose()?
        .is_some();
    let persisted_binding = CredentialBindingStore::new(home).load(&request.credential)?;
    Ok(CredentialInspection {
        resolution,
        persisted_binding,
        native_provider: native_descriptor,
        env_available,
    })
}

pub fn setup(home: &AikitHome, request: &CredentialRequest) -> Result<CredentialSetupOutcome> {
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
    eprintln!("{}", render_credential_setup_panel(&view));
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

fn selected_outcome(
    home: &AikitHome,
    request: &CredentialRequest,
    resolution: CredentialResolution,
    newly_bound: bool,
) -> Result<CredentialSetupOutcome> {
    let selected = resolution.selected_provider_ref.as_ref().ok_or_else(|| {
        AikitError::new("credential.unresolved", "credential resolution selected no provider")
    })?;
    let native = NativeSecureStoreProvider::new();
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
            format!("selected provider {} is not wired into this setup flow", selected.as_str()),
        ));
    };
    // Persist only durable provider bindings. Environment import is deliberately
    // transient and must be explicitly selected again on a later invocation.
    if binding.provider_tier != aikit_core::credential::SecretProviderTier::ExplicitEnvironmentImport {
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
    let binding = native.bind(&request.credential, &secret)?;
    CredentialBindingStore::new(home).save(&binding)?;
    let resolution = resolve_registered_credential(
        requirement(request)?,
        &[&native],
        false,
        false,
    )?;
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
    let resolution = resolve_registered_credential(
        requirement(request)?,
        &[&env],
        request.headless,
        true,
    )?;
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
    let resolution = resolve_registered_credential(
        requirement(request)?,
        &[&provider],
        false,
        false,
    )?;
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
        CredentialProviderRejection::NoPermittedMaterialisation => {
            "no-permitted-materialisation"
        }
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

fn io_error(error: io::Error) -> AikitError {
    AikitError::new("credential.io_failed", format!("credential prompt I/O failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::EnvironmentImportProvider;
    use aikit_core::credential::{resolve_registered_credential, SecretProvider};

    #[test]
    fn headless_error_preserves_exact_unbound_reason() {
        let credential = CredentialRef::new("credential:test/headless").unwrap();
        let env = EnvironmentImportProvider::from_value(
            credential.clone(),
            "AIKIT_MISSING_TOKEN",
            None,
        )
        .unwrap();
        let request = CredentialRequest {
            credential,
            consumer_ref: "harness:test".into(),
            purpose: "test".into(),
            env_var: Some("AIKIT_MISSING_TOKEN".into()),
            project_env: None,
            from_env: false,
            headless: true,
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
}