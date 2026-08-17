//! Credential initial-configuration surface.
//!
//! This is deliberately businesslike: one disclosure panel shared by the TUI
//! and CLI flow. It contains no secret input and therefore remains safe to
//! snapshot, log as UI structure, or expose through tests.

use aikit_core::credential::{
    CredentialRef, SecretMaterialisationClass, SecretProviderDescriptor, SecretProviderTier,
};
use ratatui::widgets::{Block, Borders, Paragraph};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CredentialSetupView {
    pub credential_ref: CredentialRef,
    pub consumer_ref: String,
    pub purpose: String,
    pub native_provider: SecretProviderDescriptor,
    pub env_var: Option<String>,
    pub env_available: bool,
    pub encrypted_fallback_available: bool,
    pub headless: bool,
}

pub fn render_credential_setup_panel(view: &CredentialSetupView) -> String {
    let native = &view.native_provider;
    let native_status = if native.available {
        if native.supported_credentials.contains(&view.credential_ref) {
            "available · bound"
        } else {
            "available · unbound"
        }
    } else {
        "unavailable"
    };
    let materialisation = native
        .supported_materialisation
        .iter()
        .next()
        .cloned()
        .unwrap_or(SecretMaterialisationClass::ProviderNativeLease);

    let mut lines = vec![
        "Credential setup".to_string(),
        format!("Credential: {}", view.credential_ref.as_str()),
        format!("For: {}", view.purpose),
        format!("Consumer: {}", view.consumer_ref),
        format!(
            "Native store: {} · tier {} · {native_status}",
            native.provider_kind,
            tier_name(native.tier)
        ),
        format!("Materialisation: {}", materialisation_name(materialisation)),
        format!("Binding provenance: {}", native.binding_provenance),
        String::new(),
    ];

    if view.headless {
        lines.push(
            "Headless mode: no secret prompt. Resolve an existing binding or use explicit --from-env."
                .into(),
        );
        if let Some(env_var) = &view.env_var {
            lines.push(format!(
                "Explicit environment import: --from-env --env-var {env_var} ({})",
                if view.env_available {
                    "present"
                } else {
                    "not present"
                }
            ));
        }
        return lines.join("\n");
    }

    lines.push("Choose a credential source:".into());
    lines.push("  [1] Enter secret and bind it to the OS secure store".into());
    if let Some(env_var) = &view.env_var {
        lines.push(format!(
            "  [2] Import explicitly from {env_var} (--from-env) · {} · lowest tier",
            if view.env_available {
                "present"
            } else {
                "not present"
            }
        ));
    } else {
        lines.push("  [2] Import explicitly from a named environment variable (--from-env --env-var NAME) · lowest tier".into());
    }
    if view.encrypted_fallback_available {
        lines.push(
            "  [3] Use explicit encrypted local fallback (Linux; only when Secret Service is absent)"
                .into(),
        );
    }
    lines.push("  [q] Cancel".into());
    lines.join("\n")
}

pub fn credential_setup_widget(view: &CredentialSetupView) -> Paragraph<'static> {
    Paragraph::new(render_credential_setup_panel(view))
        .block(Block::default().borders(Borders::ALL).title("Credentials"))
}

fn tier_name(tier: SecretProviderTier) -> &'static str {
    match tier {
        SecretProviderTier::OsSecureStore => "os-secure-store",
        SecretProviderTier::BrokeredSecureProvider => "brokered-secure-provider",
        SecretProviderTier::ExplicitEncryptedFallback => "explicit-encrypted-fallback",
        SecretProviderTier::FederatedOrDynamic => "federated-or-dynamic",
        SecretProviderTier::ExplicitEnvironmentImport => "explicit-environment-import",
    }
}

fn materialisation_name(class: SecretMaterialisationClass) -> &'static str {
    match class {
        SecretMaterialisationClass::ProcessEnv => "process-env",
        SecretMaterialisationClass::OneShotChildProcess => "one-shot-child-process",
        SecretMaterialisationClass::FdOrPipe => "fd-or-pipe",
        SecretMaterialisationClass::FileOrTmpfsMount => "file-or-tmpfs-mount",
        SecretMaterialisationClass::ProviderNativeLease => "provider-native-lease",
        SecretMaterialisationClass::CredentialBroker => "credential-broker",
        SecretMaterialisationClass::ShortLivedFederatedCredential => {
            "short-lived-federated-credential"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_core::credential::{SecretProviderRef, SecretProviderTier};
    use std::collections::BTreeSet;

    fn view(headless: bool) -> CredentialSetupView {
        let credential_ref = CredentialRef::new("credential:openai/research").unwrap();
        CredentialSetupView {
            credential_ref: credential_ref.clone(),
            consumer_ref: "harness:pi".into(),
            purpose: "provider inference".into(),
            native_provider: SecretProviderDescriptor {
                provider_ref: SecretProviderRef::new(
                    "provider:os-secure-store/macos-keychain",
                )
                .unwrap(),
                provider_kind: "macos-keychain".into(),
                tier: SecretProviderTier::OsSecureStore,
                available: true,
                headless_capable: true,
                assurance: "fixture".into(),
                degradation: None,
                supported_credentials: BTreeSet::new(),
                supported_materialisation: [SecretMaterialisationClass::ProviderNativeLease]
                    .into_iter()
                    .collect(),
                binding_provenance:
                    "macos-keychain:service=dev.aikit.credentials;account=credential:openai/research"
                        .into(),
                revision_or_lease_class: Some("keyring-v1/4.1.5".into()),
            },
            env_var: Some("OPENAI_API_KEY".into()),
            env_available: true,
            encrypted_fallback_available: false,
            headless,
        }
    }

    #[test]
    fn interactive_panel_keeps_env_import_visible() {
        insta::assert_snapshot!(render_credential_setup_panel(&view(false)), @r###"
        Credential setup
        Credential: credential:openai/research
        For: provider inference
        Consumer: harness:pi
        Native store: macos-keychain · tier os-secure-store · available · unbound
        Materialisation: provider-native-lease
        Binding provenance: macos-keychain:service=dev.aikit.credentials;account=credential:openai/research

        Choose a credential source:
          [1] Enter secret and bind it to the OS secure store
          [2] Import explicitly from OPENAI_API_KEY (--from-env) · present · lowest tier
          [q] Cancel
        "###);
    }

    #[test]
    fn headless_panel_never_offers_secret_prompt() {
        let rendered = render_credential_setup_panel(&view(true));
        assert!(!rendered.contains("Enter secret"));
        assert!(rendered.contains("no secret prompt"));
        assert!(rendered.contains("--from-env"));
    }
}
