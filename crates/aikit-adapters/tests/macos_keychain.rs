#![cfg(target_os = "macos")]

use std::time::{SystemTime, UNIX_EPOCH};

use aikit_adapters::{NativeSecureStoreProvider, NativeSecureStoreStatus};
use aikit_core::credential::{
    CredentialRef, SecretMaterialisationClass, SecretProvider, SecretProviderTier, SecretValue,
};

#[test]
fn macos_keychain_round_trip_skip_if_unavailable() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let credential = CredentialRef::new(format!(
        "credential:test/macos-keychain/{}-{nonce}",
        std::process::id()
    ))
    .unwrap();
    let provider = NativeSecureStoreProvider::new();

    if provider.status(&credential) == NativeSecureStoreStatus::Unavailable {
        eprintln!("skipping macOS Keychain integration test: native store unavailable");
        return;
    }

    let secret = SecretValue::new("aikit-keychain-integration-fixture").unwrap();
    let state = provider.bind(&credential, &secret).unwrap();
    assert_eq!(state.provider_tier, SecretProviderTier::OsSecureStore);
    assert_eq!(
        state.materialisation,
        SecretMaterialisationClass::ProviderNativeLease
    );
    assert!(state.binding_provenance.contains("macos-keychain"));

    let materialised = provider
        .materialise(&credential, SecretMaterialisationClass::ProviderNativeLease)
        .unwrap()
        .unwrap();
    assert_eq!(materialised.expose(), secret.expose());

    provider.delete(&credential).unwrap();
}
