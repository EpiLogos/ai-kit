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
    let state = match provider.bind(&credential, &secret) {
        Ok(state) => state,
        Err(error) if error.code() == "credential.native_store_unavailable" => {
            eprintln!(
                "skipping macOS Keychain integration test: native store unavailable: {error}"
            );
            return;
        }
        Err(error) => panic!("macOS Keychain bind failed: {error}"),
    };
    assert_eq!(state.provider_tier, SecretProviderTier::OsSecureStore);
    assert_eq!(
        state.materialisation,
        SecretMaterialisationClass::ProviderNativeLease
    );
    assert!(state.binding_provenance.contains("macos-keychain"));

    let materialised = match provider
        .materialise(&credential, SecretMaterialisationClass::ProviderNativeLease)
    {
        Ok(Some(materialised)) => materialised,
        Ok(None) => panic!("macOS Keychain binding disappeared before materialisation"),
        Err(error) if error.code() == "credential.native_store_unavailable" => {
            eprintln!(
                "skipping macOS Keychain integration test after bind: native store unavailable: {error}"
            );
            let _ = provider.delete(&credential);
            return;
        }
        Err(error) => panic!("macOS Keychain materialisation failed: {error}"),
    };
    assert_eq!(materialised.expose(), secret.expose());

    match provider.delete(&credential) {
        Ok(()) => {}
        Err(error) if error.code() == "credential.native_store_unavailable" => {
            eprintln!(
                "macOS Keychain became unavailable during cleanup; fixture may remain: {error}"
            );
        }
        Err(error) => panic!("macOS Keychain cleanup failed: {error}"),
    }
}
