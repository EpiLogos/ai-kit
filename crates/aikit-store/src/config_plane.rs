//! Owner-side receipts and idempotency for the O:I configuration plane.
//!
//! The configuration plane (#299 C0, `09-CONFIGURATION-PLANE.md`) makes the
//! owner the record of record for mutations driven through `aikit config
//! apply|reset`: O:I stores only the receipt reference, so AIKit must keep the
//! receipts themselves and enforce the idempotency contract owner-side.
//!
//! Two facts are persisted under `<home>/state/config/`:
//!
//! * `receipts.jsonl` — one `oi.config-receipt/v1` document per line, append
//!   only. This is the owner-native history the receipt's `native_ref` points
//!   into.
//! * Replays are answered by scanning that same file for the frozen
//!   idempotency key `(owner_ref, changeset_id, setting_ref, scope,
//!   plan_digest)` among executed receipts. A replay is never re-executed and
//!   never recorded as a new executed key — it returns `no_op` naming the
//!   original receipt.
//!
//! No secret material can enter this file: receipts carry setting identity,
//! scope, digests and refs, never values.

use std::io::Write;
use std::path::PathBuf;

use aikit_core::AikitError;

use crate::home::AikitHome;

/// One idempotency lookup: the frozen key fields of an executed mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutedKey {
    pub owner_ref: String,
    pub changeset_id: String,
    pub setting_ref: String,
    pub scope_kind: String,
    /// `None` for singular scope kinds (`machine`).
    pub scope_ref: Option<String>,
    /// `None` for `reset`, which executes without a plan.
    pub plan_digest: Option<String>,
}

/// The owner-native receipt history for configuration-plane mutations.
pub struct ConfigReceiptStore {
    path: PathBuf,
}

impl ConfigReceiptStore {
    pub fn new(home: &AikitHome) -> Self {
        Self {
            path: home.config_plane().join("receipts.jsonl"),
        }
    }

    /// Append one receipt to the owner history, creating the directory on
    /// first write.
    pub fn record(&self, receipt: &serde_json::Value) -> Result<(), AikitError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                AikitError::new(
                    "config.receipt_unwritable",
                    format!("could not create {}: {error}", parent.display()),
                )
            })?;
        }
        let mut line = serde_json::to_string(receipt).map_err(|error| {
            AikitError::new(
                "config.receipt_encode_failed",
                format!("could not encode config receipt: {error}"),
            )
        })?;
        line.push('\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| {
                AikitError::new(
                    "config.receipt_unwritable",
                    format!("could not open {}: {error}", self.path.display()),
                )
            })?;
        file.write_all(line.as_bytes()).map_err(|error| {
            AikitError::new(
                "config.receipt_unwritable",
                format!("could not append to {}: {error}", self.path.display()),
            )
        })?;
        Ok(())
    }

    /// The executed receipt for this key, if a mutation already ran under it.
    /// Replays of replays answer with the same original: `no_op` receipts are
    /// not recorded as executed keys.
    pub fn find_executed(
        &self,
        key: &ExecutedKey,
    ) -> Result<Option<serde_json::Value>, AikitError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(AikitError::new(
                    "config.receipt_unreadable",
                    format!("could not read {}: {error}", self.path.display()),
                ))
            }
        };
        for line in text.lines() {
            let receipt: serde_json::Value = match serde_json::from_str(line) {
                Ok(value) => value,
                Err(_) => continue, // a torn line never fabricates a match
            };
            if receipt.get("outcome").and_then(|o| o.as_str()) != Some("applied") {
                continue;
            }
            let scope = receipt.get("scope");
            let scope_ref = scope
                .and_then(|s| s.get("scope_ref"))
                .and_then(|r| r.as_str())
                .map(str::to_string);
            let matches = receipt.get("owner_ref").and_then(|v| v.as_str())
                == Some(key.owner_ref.as_str())
                && receipt.get("changeset_id").and_then(|v| v.as_str())
                    == Some(key.changeset_id.as_str())
                && receipt.get("setting_ref").and_then(|v| v.as_str())
                    == Some(key.setting_ref.as_str())
                && scope
                    .and_then(|s| s.get("scope_kind"))
                    .and_then(|v| v.as_str())
                    == Some(key.scope_kind.as_str())
                && scope_ref == key.scope_ref
                && receipt
                    .get("plan_digest")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    == key.plan_digest;
            if matches {
                return Ok(Some(receipt));
            }
        }
        Ok(None)
    }
}
