#!/usr/bin/env python3
"""Temporary cloud codemod for precise native secure-store availability errors."""
from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if new in text:
        return
    if old not in text:
        raise SystemExit(f"missing patch anchor in {path}: {old[:100]!r}")
    p.write_text(text.replace(old, new, 1))


provider = "crates/aikit-adapters/src/credential_provider.rs"
replace_once(
    provider,
    "use std::path::{Path, PathBuf};\n",
    "use std::path::Path;\n#[cfg(target_os = \"linux\")]\nuse std::path::PathBuf;\n",
)
replace_once(
    provider,
    '''fn provider_error(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}
''',
    '''fn provider_error(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

fn native_store_unavailable(error: &KeyringError) -> bool {
    matches!(
        error,
        KeyringError::NoStorageAccess(_)
            | KeyringError::NoDefaultStore
            | KeyringError::PlatformFailure(_)
    )
}

fn native_store_operation_error(
    unavailable_message: &str,
    operation_code: &'static str,
    operation_message: &str,
    error: KeyringError,
) -> AikitError {
    if native_store_unavailable(&error) {
        provider_error(
            "credential.native_store_unavailable",
            format!("{unavailable_message}: {error}"),
        )
    } else {
        provider_error(operation_code, format!("{operation_message}: {error}"))
    }
}
''',
)

replace_once(
    provider,
    '''        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(provider_error(
                "credential.native_store_delete_failed",
                format!("could not delete native credential binding: {error}"),
            )),
        }
''',
    '''        match entry.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(error) => Err(native_store_operation_error(
                "native credential store is not accessible",
                "credential.native_store_delete_failed",
                "could not delete native credential binding",
                error,
            )),
        }
''',
)

replace_once(
    provider,
    '''        entry.set_password(secret.expose()).map_err(|error| {
            provider_error(
                "credential.native_store_write_failed",
                format!("could not bind credential in native secure store: {error}"),
            )
        })?;
''',
    '''        entry.set_password(secret.expose()).map_err(|error| {
            native_store_operation_error(
                "native credential store is not accessible",
                "credential.native_store_write_failed",
                "could not bind credential in native secure store",
                error,
            )
        })?;
''',
)

replace_once(
    provider,
    '''        match entry.get_password() {
            Ok(secret) => SecretValue::new(secret).map(Some),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(provider_error(
                "credential.native_store_read_failed",
                format!("could not materialise native credential: {error}"),
            )),
        }
''',
    '''        match entry.get_password() {
            Ok(secret) => SecretValue::new(secret).map(Some),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(error) => Err(native_store_operation_error(
                "native credential store is not accessible",
                "credential.native_store_read_failed",
                "could not materialise native credential",
                error,
            )),
        }
''',
)
