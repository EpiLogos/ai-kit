#!/usr/bin/env python3
"""Temporary cloud codemod for compile-clean provider details."""
from pathlib import Path

path = Path("crates/aikit-adapters/src/credential_provider.rs")
text = path.read_text()

# The earlier idempotent transform intentionally handled one shell-env initializer;
# ensure the project-env initializer gets the same non-secret availability marker.
old = '''                    env_var,
                    value: Some(SecretValue::new(value)?),
                });'''
new = '''                    env_var,
                    source_available: true,
                    value: Some(SecretValue::new(value)?),
                });'''
text = text.replace(old, new)

old_decrypt = '''            let plaintext = cipher
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )'''
new_decrypt = '''            let nonce = XNonce::try_from(nonce.as_slice()).map_err(|_| {
                provider_error(
                    "credential.encrypted_fallback_invalid",
                    "encrypted fallback nonce must be 24 bytes",
                )
            })?;
            let plaintext = cipher
                .decrypt(
                    &nonce,
                    Payload {
                        msg: &ciphertext,
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )'''
if new_decrypt not in text:
    if old_decrypt not in text:
        raise SystemExit("decrypt nonce anchor missing")
    text = text.replace(old_decrypt, new_decrypt, 1)

old_encrypt = '''            let ciphertext = cipher
                .encrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: secret.expose().as_bytes(),
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )'''
new_encrypt = '''            let nonce_value = XNonce::try_from(nonce.as_slice()).expect("fixed 24-byte nonce");
            let ciphertext = cipher
                .encrypt(
                    &nonce_value,
                    Payload {
                        msg: secret.expose().as_bytes(),
                        aad: credential_ref.as_str().as_bytes(),
                    },
                )'''
if new_encrypt not in text:
    if old_encrypt not in text:
        raise SystemExit("encrypt nonce anchor missing")
    text = text.replace(old_encrypt, new_encrypt, 1)

path.write_text(text)
