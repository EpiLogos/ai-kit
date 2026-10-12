//! Sender attestation (#481-8): a relayed Communique's sender claim is
//! signed by the relaying gateway's own Ed25519 key.
//!
//! Before this, a relayed `send` carried a self-declared sender authenticated
//! only by the machine-level peer token — the same trust as the old ssh
//! route: any machine holding that token could claim to be any agency. With
//! attestation, the relaying gateway signs WHAT it asserts about the sender
//! (position, generation, attribution, the body's digest) with a private key
//! that lives only in its own AIKit home; the receiver verifies against the
//! public key, which the sender's protocol answer advertises and which an
//! operator can PIN in the remote declaration. A peer-token holder alone can
//! no longer impersonate an agency.
//!
//! Trust anchor, named honestly: the public key is learned from the sender's
//! protocol answer (trust on first use) and can be pinned per remote. A
//! rotated key that is not the pinned key is refused. An absent attestation
//! is today's behaviour, unchanged and named — never silently upgraded.

use crate::gateway_communique::Communique;
use aikit_core::{AikitError, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

/// How old a carried attestation may be when it is verified. A relay is a
/// near-real-time act; a stale attestation is a replay, not a proof.
pub const ATTESTATION_MAX_AGE_MS: u64 = 10 * 60 * 1000;

/// What the relaying gateway asserts about the sender. The digest binds the
/// attestation to THIS body: a captured attestation cannot be re-attached to
/// different content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderAttestationBody {
    pub communique_ref: String,
    pub from_position_ref: String,
    pub from_generation_ref: String,
    pub attribution: String,
    pub body_sha256: String,
    pub sent_at_unix_ms: u64,
}

/// The carried proof: the body, the signing gateway, its public key, and the
/// signature over the canonical body encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SenderAttestation {
    pub body: SenderAttestationBody,
    /// The relaying gateway, as its own protocol answer names itself.
    pub gateway_ref: String,
    /// Ed25519 public key, hex — the same string the sender's protocol answer
    /// advertises as `sender_attestation_key`.
    pub public_key: String,
    /// Ed25519 signature over the canonical body encoding, hex.
    pub signature: String,
    pub signed_at_unix_ms: u64,
}

fn canonical(body: &SenderAttestationBody) -> Result<Vec<u8>> {
    // Field order fixed by the struct; serde_json is canonical for these
    // value types (strings, integers — no floats, no maps).
    serde_json::to_vec(body)
        .map_err(|error| AikitError::new("gateway_attestation.encode", error.to_string()))
}

fn body_sha256(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// This gateway's long-lived signing key: generated on first use, persisted
/// owner-only beside the gateway state, stable across restarts so a pinned
/// key stays pinned.
pub fn load_or_create_signing_key(state_dir: &Path) -> Result<SigningKey> {
    let path = state_dir.join("gateway-signing.key");
    if let Ok(bytes) = std::fs::read(&path) {
        if bytes.len() == 32 {
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes);
            return Ok(SigningKey::from_bytes(&seed));
        }
        return Err(AikitError::new(
            "gateway_attestation.keyfile_invalid",
            format!(
                "{} exists but is not a 32-byte signing seed; it was not overwritten",
                path.display()
            ),
        ));
    }
    let mut seed = [0u8; 32];
    // The workspace's own entropy source (the credential provider uses the
    // same call): fill the seed from the OS, then hand it to dalek.
    getrandom::fill(&mut seed)
        .map_err(|error| AikitError::new("gateway_attestation.entropy", error.to_string()))?;
    let key = SigningKey::from_bytes(&seed);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AikitError::new(
                "gateway_attestation.io",
                format!("create {}: {error}", parent.display()),
            )
        })?;
    }
    std::fs::write(&path, seed).map_err(|error| {
        AikitError::new(
            "gateway_attestation.io",
            format!("write {}: {error}", path.display()),
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                AikitError::new(
                    "gateway_attestation.io",
                    format!("chmod 600 {}: {error}", path.display()),
                )
            },
        )?;
    }
    Ok(key)
}

/// The public key a gateway's protocol answer advertises, hex.
pub fn public_key_hex(key: &SigningKey) -> String {
    hex(key.verifying_key().as_bytes())
}

/// Sign what this gateway asserts about a Communique it is about to relay.
pub fn attest(
    key: &SigningKey,
    gateway_ref: &str,
    communique: &Communique,
    now_unix_ms: u64,
) -> Result<SenderAttestation> {
    let body = SenderAttestationBody {
        communique_ref: communique.communique_ref.clone(),
        from_position_ref: communique.from_position_ref.clone().unwrap_or_default(),
        from_generation_ref: communique.from_generation_ref.clone().unwrap_or_default(),
        attribution: format!("{:?}", communique.attribution),
        body_sha256: body_sha256(&communique.body),
        sent_at_unix_ms: communique.sent_at_unix_ms,
    };
    let signature = key.sign(&canonical(&body)?);
    Ok(SenderAttestation {
        body,
        gateway_ref: gateway_ref.to_owned(),
        public_key: public_key_hex(key),
        signature: hex(&signature.to_bytes()),
        signed_at_unix_ms: now_unix_ms,
    })
}

/// Verify a carried attestation against a Communique. `pinned_key` is the
/// operator's pin from the remote declaration, when one exists — a carried
/// key that differs from the pin is refused (a pinned key must mean the
/// pinned machine).
pub fn verify(
    communique: &Communique,
    attestation: &SenderAttestation,
    now_unix_ms: u64,
    pinned_key: Option<&str>,
) -> Result<()> {
    if let Some(pin) = pinned_key {
        if !attestation.public_key.eq_ignore_ascii_case(pin.trim()) {
            return Err(AikitError::new(
                "gateway_attestation.key_not_pinned",
                format!(
                    "the attestation of {} carries public key {} but {} is pinned for gateway {}; \
                     a rotated or forged key is not accepted",
                    attestation.body.communique_ref,
                    attestation.public_key,
                    pin.trim(),
                    attestation.gateway_ref
                ),
            ));
        }
    }
    if now_unix_ms.saturating_sub(attestation.signed_at_unix_ms) > ATTESTATION_MAX_AGE_MS {
        return Err(AikitError::new(
            "gateway_attestation.stale",
            format!(
                "the attestation of {} was signed {} ms ago; past {} ms it is a replay, not a proof",
                attestation.body.communique_ref,
                now_unix_ms.saturating_sub(attestation.signed_at_unix_ms),
                ATTESTATION_MAX_AGE_MS
            ),
        ));
    }
    let expected = SenderAttestationBody {
        communique_ref: communique.communique_ref.clone(),
        from_position_ref: communique.from_position_ref.clone().unwrap_or_default(),
        from_generation_ref: communique.from_generation_ref.clone().unwrap_or_default(),
        attribution: format!("{:?}", communique.attribution),
        body_sha256: body_sha256(&communique.body),
        sent_at_unix_ms: communique.sent_at_unix_ms,
    };
    if attestation.body != expected {
        return Err(AikitError::new(
            "gateway_attestation.body_mismatch",
            format!(
                "the attestation of {} does not describe the Communique as received: the sender \
                 claim, the body digest, or the timestamps differ",
                communique.communique_ref
            ),
        ));
    }
    let mut key_bytes = [0u8; 32];
    hex_decode_32(&attestation.public_key).map(|bytes| key_bytes = bytes)?;
    let verifying = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|error| AikitError::new("gateway_attestation.key_invalid", error.to_string()))?;
    let mut sig_bytes = [0u8; 64];
    hex_decode_64(&attestation.signature).map(|bytes| sig_bytes = bytes)?;
    let signature = Signature::from_bytes(&sig_bytes);
    verifying
        .verify(&canonical(&expected)?, &signature)
        .map_err(|error| {
            AikitError::new(
                "gateway_attestation.signature_invalid",
                format!(
                    "the signature on {} does not verify against the carried public key: {error}",
                    communique.communique_ref
                ),
            )
        })
}

/// Whether an ingested Communique's sender claim is upgraded by a verified
/// attestation. The stored record stays honest either way: an attested relay
/// says so in its attribution basis; an unattested one keeps today's
/// self-declared basis, named.
pub fn attested_basis(attestation: Option<&SenderAttestation>) -> String {
    match attestation {
        Some(proof) => format!(
            "sender attested by gateway {} (ed25519 signature verified against {})",
            proof.gateway_ref, proof.public_key
        ),
        None => {
            "self-declared sender, authenticated only by the machine-level peer token".to_owned()
        }
    }
}

fn hex_decode_32(text: &str) -> Result<[u8; 32]> {
    decode_into::<32>(text)
}

fn hex_decode_64(text: &str) -> Result<[u8; 64]> {
    decode_into::<64>(text)
}

fn decode_into<const N: usize>(text: &str) -> Result<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AikitError::new(
            "gateway_attestation.hex_invalid",
            format!(
                "expected {N} hex-encoded bytes, got {} characters",
                text.len()
            ),
        ));
    }
    let mut out = [0u8; N];
    for (index, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).map_err(|error| {
            AikitError::new("gateway_attestation.hex_invalid", error.to_string())
        })?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway_communique::{Communique, SenderAttribution};

    fn communique(body: &str) -> Communique {
        serde_json::from_value(serde_json::json!({
            "schema": "aikit.communique/v1",
            "communique_ref": "communique:01attest",
            "sequence": 1,
            "from_position_ref": "position:sender",
            "from_generation_ref": "gen-1",
            "attribution": "verified",
            "attribution_basis": "actuation occupancy verify",
            "to_position_ref": "position:steward",
            "body": body,
            "sent_at_unix_ms": 1000,
            "state": "pending",
            "origin_gateway_ref": "agency-gateway/sender",
        }))
        .unwrap()
    }

    #[test]
    fn an_attestation_verifies_and_binds_the_body_it_signs() {
        let dir = tempfile::tempdir().unwrap();
        let key = load_or_create_signing_key(dir.path()).unwrap();
        let record = communique("hello across the machines");
        let proof = attest(&key, "agency-gateway/sender", &record, 5000).unwrap();
        verify(&record, &proof, 6000, None).expect("a fresh, honest attestation verifies");
        // A different body is refused: the digest binds the proof to content.
        let other = communique("hello across the machines (edited)");
        assert_eq!(
            verify(&other, &proof, 6000, None).err().unwrap().code(),
            "gateway_attestation.body_mismatch"
        );
        // A replayed (stale) proof is refused.
        assert_eq!(
            verify(&record, &proof, 5000 + ATTESTATION_MAX_AGE_MS + 1, None)
                .err()
                .unwrap()
                .code(),
            "gateway_attestation.stale"
        );
    }

    #[test]
    fn a_forged_or_rotated_key_is_refused_and_a_pin_holds() {
        let dir = tempfile::tempdir().unwrap();
        let key = load_or_create_signing_key(dir.path()).unwrap();
        let impostor_dir = tempfile::tempdir().unwrap();
        let impostor = load_or_create_signing_key(impostor_dir.path()).unwrap();
        let record = communique("impersonate me");
        let proof = attest(&impostor, "agency-gateway/sender", &record, 5000).unwrap();
        // The forged signature does not verify against a DIFFERENT honest key.
        let honest = attest(&key, "agency-gateway/sender", &record, 5000).unwrap();
        let mut forged = proof.clone();
        forged.public_key = honest.public_key.clone();
        assert_eq!(
            verify(&record, &forged, 6000, None).err().unwrap().code(),
            "gateway_attestation.signature_invalid"
        );
        // A pinned key must mean the pinned machine: a carried key that is
        // not the pin is refused before any signature is read.
        assert_eq!(
            verify(&record, &proof, 6000, Some(&honest.public_key))
                .err()
                .unwrap()
                .code(),
            "gateway_attestation.key_not_pinned"
        );
        verify(&record, &proof, 6000, Some(&proof.public_key))
            .expect("the pin matching the carried key verifies");
    }

    #[test]
    fn the_signing_key_is_stable_across_restarts_and_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create_signing_key(dir.path()).unwrap();
        let second = load_or_create_signing_key(dir.path()).unwrap();
        assert_eq!(public_key_hex(&first), public_key_hex(&second));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("gateway-signing.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "the signing seed is owner-only");
        }
    }

    #[test]
    fn the_ingest_upgrades_an_attested_sender_claim_and_refuses_a_bad_one() {
        use crate::gateway_runtime::{execute_gateway_command, AgencyGateway};
        use aikit_core::resource::ResourceRef;
        let mut gateway =
            AgencyGateway::new(ResourceRef::parse("agency-gateway/receiver").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let key = load_or_create_signing_key(dir.path()).unwrap();
        let communique = communique("relay me");
        let now = crate::gateway_posture::unix_ms_now();
        let proof = attest(&key, "agency-gateway/sender", &communique, now).unwrap();
        let accepted = execute_gateway_command(
            &mut gateway,
            crate::gateway_runtime::GatewayCommand::IngestCommunique {
                communique: Box::new(communique.clone()),
                relayed_by: "agency-gateway/sender".into(),
                attestation: Some(proof),
            },
        )
        .unwrap();
        let crate::gateway_runtime::GatewayResponse::CommuniqueAccepted { communique, .. } =
            accepted
        else {
            panic!("expected an accepted communique")
        };
        assert!(
            communique.attribution_basis.contains("sender attested"),
            "the stored record names the attestation: {}",
            communique.attribution_basis
        );
        // A tampered body is refused at the gate: nothing is recorded.
        let mut tampered = communique_for_gate("relay me");
        tampered.body = "relay me (changed)".into();
        let proof = attest(&key, "agency-gateway/sender", &tampered, now).unwrap();
        tampered.body = "relay me".into();
        let refused = execute_gateway_command(
            &mut gateway,
            crate::gateway_runtime::GatewayCommand::IngestCommunique {
                communique: Box::new(tampered),
                relayed_by: "agency-gateway/sender".into(),
                attestation: Some(proof),
            },
        );
        assert_eq!(
            refused.err().unwrap().code(),
            "gateway_attestation.body_mismatch"
        );
    }

    fn communique_for_gate(body: &str) -> Communique {
        communique(body)
    }

    // SenderAttribution is exercised through the fixture's attribution field.
    #[test]
    fn attribution_wording_is_stable() {
        assert_eq!(
            attested_basis(None),
            "self-declared sender, authenticated only by the machine-level peer token"
        );
        let _ = SenderAttribution::Verified;
    }
}
