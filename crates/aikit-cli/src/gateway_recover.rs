//! `aikit gateway recover` — a gateway whose state file will not load, and
//! the delivery ledger nothing else names.
//!
//! The service restores `state/gateway.json` at start and refuses to run from a
//! file it cannot decode (a torn write on a full disk, a hand edit). Before this
//! the operator's only move was to delete the file and lose every Communique.
//! Recovery is plan-first and loses nothing it does not name:
//!
//! 1. the damaged file is **quarantined**, never deleted (`gateway.json.damaged-<ms>`);
//! 2. the newest copy that *does* load is restored from: an upgrade's recovery
//!    basis (`state/gateway-upgrade/<id>/recovery/gateway.json`), else a leftover
//!    atomic-write temporary that parses;
//! 3. with no copy, the gateway starts empty and says so — the quarantined file
//!    is still there to read by hand.
//!
//! The same command reads the gateway's **pending deliveries** (`--deliveries`):
//! every prepared, unreceipted outbound operation, with its attempt evidence.
//! An attempted-but-unreceipted send is outcome-unknown — `--resolve` records
//! the receipt the owner's evidence supports (`delivered` or `abandoned`) and
//! retires the pending entry. Nothing is ever re-sent by recovery.
//!
//! State-file repair refuses while a gateway is running: the service owns the
//! file for as long as it runs. The delivery ledger reads and resolves through
//! the running gateway's owner carrier (the unix socket, whose file mode is
//! the owner scope).

use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::{
    gateway_command_within, DeliveryReceipt, DeliveryState, GatewayCarrierTarget, GatewayCommand,
    GatewayResponse, GatewaySnapshot,
};
use aikit_core::resource::ResourceRef;
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use serde_json::{json, Value};

use crate::cli::GatewayRecoverArgs;
use crate::gateway_contact::three_part;
use crate::gateway_upgrade::write_atomic;

pub const RECOVER_SCHEMA: &str = "aikit.gateway-recover/v1";

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    source: String,
    snapshot: GatewaySnapshot,
}

fn load(path: &Path) -> Option<GatewaySnapshot> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Copies that decode, newest first BY WHEN THEY WERE WRITTEN (file modification
/// time), whichever kind they are: upgrade recovery bases, or a leftover
/// atomic-write temporary. The kind decides nothing; the newest copy is the one
/// that lost the least.
fn candidates(home: &AikitHome) -> Vec<Candidate> {
    let mut found: Vec<(std::time::SystemTime, Candidate)> = Vec::new();
    let modified = |path: &Path| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .unwrap_or(std::time::UNIX_EPOCH)
    };
    let upgrades = home.state().join("gateway-upgrade");
    if let Ok(entries) = std::fs::read_dir(&upgrades) {
        for dir in entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
            let copy = dir.join("recovery/gateway.json");
            if let Some(snapshot) = load(&copy) {
                let id = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                found.push((
                    modified(&copy),
                    Candidate {
                        path: copy,
                        source: format!("the recovery basis of upgrade {id}"),
                        snapshot,
                    },
                ));
            }
        }
    }
    let state = home.gateway_state();
    let temporary = state.with_extension("json.tmp");
    if let Some(snapshot) = load(&temporary) {
        found.push((
            modified(&temporary),
            Candidate {
                path: temporary,
                source: "an unfinished atomic write that still decodes".to_owned(),
                snapshot,
            },
        ));
    }
    // Newest first; the path breaks a tie so the order is deterministic.
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.path.cmp(&a.1.path)));
    found.into_iter().map(|(_, candidate)| candidate).collect()
}

fn gateway_answers(home: &AikitHome) -> bool {
    gateway_command_within(
        &GatewayCarrierTarget::UnixSocket(home.gateway_socket()),
        GatewayCommand::Protocol,
        None,
        Duration::from_secs(2),
    )
    .is_ok()
}

/// The pending outbound operations this gateway still owes: prepared, not
/// receipted. A send whose outcome is unknown (attempted, no receipt) is
/// named for evidence resolution — never blindly re-sent; the idempotent
/// kinds were already re-attempted by the connector pump on its reconnect.
pub fn pending_deliveries(home: &AikitHome) -> Result<Value> {
    let target = owner_carrier(home);
    let response = aikit_adapters::gateway_command(
        &target,
        GatewayCommand::PendingDeliveries { connector_ref: None },
        None,
    )?;
    let GatewayResponse::PendingDeliveries { operations } = response else {
        return Err(AikitError::new(
            "gateway.recover_unexpected_answer",
            "the gateway answered the pending-deliveries read with something else",
        ));
    };
    let listed: Vec<Value> = operations
        .iter()
        .map(|operation| {
            json!({
                "operation_ref": operation.operation_ref.to_string(),
                "connector_ref": operation.connector_ref.to_string(),
                "platform": operation.address.platform,
                "conversation_id": operation.address.conversation_id,
                "attempts": operation.attempts,
                "last_attempt_at_unix_ms": operation.last_attempt_at_unix_ms,
                "outcome": if operation.attempts > 0 {
                    "attempted, outcome unknown: resolve by evidence or leave it held"
                } else {
                    "prepared, never attempted: idempotent kinds re-attempt on connector reconnect; sends hold"
                },
            })
        })
        .collect();
    Ok(json!({
        "schema": RECOVER_SCHEMA,
        "reading": "pending-deliveries",
        "pending_count": listed.len(),
        "pending": listed,
        "law": "an unreceipted operation is re-attempted only when idempotent; a send is resolved by evidence (`--resolve`), never blindly re-sent",
    }))
}

/// Resolve one pending delivery by evidence: record the receipt the evidence
/// supports and retire the pending entry. Nothing is re-sent.
pub fn resolve_delivery(
    home: &AikitHome,
    operation_ref: &str,
    state: &str,
    evidence: Option<&str>,
) -> Result<Value> {
    let delivery_state = match state {
        "delivered" => DeliveryState::Delivered,
        "abandoned" => DeliveryState::Failed,
        other => {
            return Err(AikitError::new(
                "gateway.recover_state_invalid",
                format!(
                    "`{other}` is not a delivery resolution: use `delivered` or `abandoned`"
                ),
            ))
        }
    };
    let reference = ResourceRef::parse(operation_ref).map_err(|error| {
        AikitError::new(
            "gateway.recover_ref_invalid",
            format!("{operation_ref} is not an operation ref: {error}"),
        )
    })?;
    let target = owner_carrier(home);
    let mut provenance = vec![
        "resolved by owner evidence through `aikit gateway recover --resolve`".to_owned(),
    ];
    if let Some(evidence) = evidence {
        provenance.push(format!("evidence: {evidence}"));
    }
    let receipt = DeliveryReceipt {
        operation_ref: reference.clone(),
        connector_ref: ResourceRef::parse("gateway-connector/resolved-by-evidence")
            .expect("static ref parses"),
        state: delivery_state,
        native_message_id: None,
        detail: Some(evidence.unwrap_or("the owner resolved it by evidence").to_owned()),
        native: Default::default(),
        provenance: provenance.clone(),
    };
    aikit_adapters::gateway_command(&target, GatewayCommand::RecordDelivery { receipt }, None)?;
    Ok(json!({
        "schema": RECOVER_SCHEMA,
        "reading": "delivery-resolved",
        "operation_ref": reference.to_string(),
        "state": state,
        "evidence": evidence,
        "note": "recorded as a receipt; the pending entry is retired. Nothing was re-sent",
    }))
}

/// The owner carrier of this home: the unix socket, whose file mode is the
/// owner scope.
fn owner_carrier(home: &AikitHome) -> GatewayCarrierTarget {
    GatewayCarrierTarget::UnixSocket(home.gateway_socket())
}

/// `aikit gateway recover`: state-file repair (the default), the delivery
/// ledger (`--deliveries`), or an evidence resolution (`--resolve …`).
pub fn recover(home: &AikitHome, args: &GatewayRecoverArgs) -> Result<Value> {
    if args.deliveries {
        return pending_deliveries(home);
    }
    if let (Some(operation), Some(state)) = (&args.resolve, &args.resolve_state) {
        return resolve_delivery(home, operation, state, args.evidence.as_deref());
    }
    let apply = args.apply;
    let state = home.gateway_state();
    if gateway_answers(home) {
        return Err(three_part(
            "gateway.recover_running",
            "A gateway is running on this home: it owns its state file while it runs.",
            "Nothing was changed.",
            "If it runs, its state loaded; to recover a file it cannot load, stop the service first (`aikit gateway uninstall-service`, or stop `aikit gateway serve`).",
        ));
    }
    let exists = state.exists();
    let loads = exists && load(&state).is_some();
    if loads {
        return Ok(json!({
            "schema": RECOVER_SCHEMA,
            "state": state.display().to_string(),
            "status": "nothing-to-recover",
            "note": "the state file decodes; the gateway can start from it",
        }));
    }
    if !exists {
        return Ok(json!({
            "schema": RECOVER_SCHEMA,
            "state": state.display().to_string(),
            "status": "no-state-file",
            "note": "there is no state file: the gateway starts empty, which is correct for a new home",
        }));
    }
    let found = candidates(home);
    let chosen = found.first();
    let quarantine = state.with_file_name(format!(
        "gateway.json.damaged-{}",
        aikit_adapters::gateway_posture::unix_ms_now()
    ));
    let plan = json!({
        "schema": RECOVER_SCHEMA,
        "state": state.display().to_string(),
        "status": if apply { "recovered" } else { "plan" },
        "damaged_file": {"quarantine_to": quarantine.display().to_string(), "deleted": false},
        "restore_from": chosen.map(|c| json!({
            "file": c.path.display().to_string(),
            "source": c.source,
            "communiques": c.snapshot.communiques.len(),
            "connectors": c.snapshot.connectors.len(),
            "bindings": c.snapshot.bindings.len(),
        })),
        "others_available": found.len().saturating_sub(1),
        "lost": if chosen.is_some() {
            "whatever the gateway recorded after that copy was taken; the quarantined file keeps any bytes that are still readable"
        } else {
            "everything in the damaged file, as far as the gateway is concerned: it starts empty (the quarantined file keeps the bytes)"
        },
    });
    if !apply {
        let mut plan = plan;
        plan["next"] = json!("aikit gateway recover --apply");
        return Ok(plan);
    }
    std::fs::rename(&state, &quarantine).map_err(|error| {
        AikitError::new(
            "gateway.recover_io",
            format!("quarantine {}: {error}", state.display()),
        )
    })?;
    if let Some(candidate) = chosen {
        let bytes = std::fs::read(&candidate.path).map_err(|error| {
            AikitError::new(
                "gateway.recover_io",
                format!("read {}: {error}", candidate.path.display()),
            )
        })?;
        write_atomic(&state, &bytes)?;
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aikit_adapters::AgencyGateway;
    use aikit_core::resource::ResourceRef;

    fn home() -> (tempfile::TempDir, AikitHome) {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().to_path_buf());
        std::fs::create_dir_all(home.state()).unwrap();
        (dir, home)
    }

    fn snapshot_bytes(reference: &str) -> Vec<u8> {
        let gateway = AgencyGateway::new(ResourceRef::parse(reference).unwrap());
        serde_json::to_vec(&gateway.snapshot()).unwrap()
    }

    #[test]
    fn a_loadable_state_file_or_none_needs_no_recovery() {
        let (_dir, home) = home();
        assert_eq!(recover(&home, false).unwrap()["status"], "no-state-file");
        std::fs::write(home.gateway_state(), snapshot_bytes("agency-gateway/a")).unwrap();
        assert_eq!(
            recover(&home, true).unwrap()["status"],
            "nothing-to-recover"
        );
    }

    #[test]
    fn a_damaged_file_is_quarantined_not_deleted_and_the_newest_loadable_copy_is_restored() {
        let (_dir, home) = home();
        std::fs::write(home.gateway_state(), b"{\"version\": \"aikit.agency-gate").unwrap();
        // Two upgrades kept recovery bases; the newer wins.
        for (id, reference) in [
            ("upg-001", "agency-gateway/old"),
            ("upg-002", "agency-gateway/new"),
        ] {
            let dir = home
                .state()
                .join("gateway-upgrade")
                .join(id)
                .join("recovery");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("gateway.json"), snapshot_bytes(reference)).unwrap();
        }
        // A plan changes nothing and says what it would do.
        let plan = recover(&home, false).unwrap();
        assert_eq!(plan["status"], "plan");
        assert_eq!(plan["damaged_file"]["deleted"], false);
        assert!(plan["restore_from"]["source"]
            .as_str()
            .unwrap()
            .contains("upg-002"));
        assert_eq!(plan["others_available"], 1);
        assert!(std::fs::read(home.gateway_state())
            .unwrap()
            .starts_with(b"{\"version\": \"aikit.agency-gate"));

        let done = recover(&home, true).unwrap();
        assert_eq!(done["status"], "recovered");
        let restored: GatewaySnapshot =
            serde_json::from_slice(&std::fs::read(home.gateway_state()).unwrap()).unwrap();
        assert_eq!(restored.gateway_ref.as_str(), "agency-gateway/new");
        let quarantined: Vec<_> = std::fs::read_dir(home.state())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("gateway.json.damaged-")
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "the damaged bytes are kept");
        assert!(std::fs::read(quarantined[0].path())
            .unwrap()
            .starts_with(b"{\"version\""));
    }

    #[test]
    fn the_newest_decodable_copy_wins_whatever_kind_it_is() {
        let (_dir, home) = home();
        std::fs::write(home.gateway_state(), b"{\"torn").unwrap();
        let basis = home.state().join("gateway-upgrade/upg-001/recovery");
        std::fs::create_dir_all(&basis).unwrap();
        std::fs::write(
            basis.join("gateway.json"),
            snapshot_bytes("agency-gateway/old"),
        )
        .unwrap();
        // The leftover temporary is written AFTER the recovery basis.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(
            home.gateway_state().with_extension("json.tmp"),
            snapshot_bytes("agency-gateway/newer"),
        )
        .unwrap();
        let plan = recover(&home, false).unwrap();
        assert!(
            plan["restore_from"]["source"]
                .as_str()
                .unwrap()
                .contains("unfinished atomic write"),
            "{plan}"
        );
        recover(&home, true).unwrap();
        let restored: GatewaySnapshot =
            serde_json::from_slice(&std::fs::read(home.gateway_state()).unwrap()).unwrap();
        assert_eq!(restored.gateway_ref.as_str(), "agency-gateway/newer");
    }

    #[test]
    fn with_no_copy_the_gateway_starts_empty_and_the_plan_says_what_is_lost() {
        let (_dir, home) = home();
        std::fs::write(home.gateway_state(), b"not json").unwrap();
        let plan = recover(&home, false).unwrap();
        assert!(plan["restore_from"].is_null());
        assert!(plan["lost"].as_str().unwrap().contains("starts empty"));
        recover(&home, true).unwrap();
        assert!(
            !home.gateway_state().exists(),
            "an empty start, the quarantine holds the bytes"
        );
    }
}
