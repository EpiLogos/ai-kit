//! Durable machine delivery in the existing encounter journal. This is transport
//! state, never human authorship, completed work, or a Factory Recognition.
use super::{failure, stamp_observed_at, validate, EncounterStore};
use aikit_core::{AikitError, ResourceRef, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncounterDelivery {
    pub agent_session: ResourceRef,
    pub delivery_ref: ResourceRef,
    pub sender: ResourceRef,
    pub request: Value,
    pub phase: String,
    pub first_cursor: u64,
    pub terminal_cursor: Option<u64>,
    pub detail: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryReservation {
    pub fresh: bool,
    pub delivery: EncounterDelivery,
}
pub(super) fn install(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS encounter_deliveries(
        session TEXT NOT NULL, delivery TEXT NOT NULL, sender TEXT NOT NULL, request TEXT NOT NULL,
        phase TEXT NOT NULL, first_cursor INTEGER NOT NULL, terminal_cursor INTEGER, detail TEXT,
        PRIMARY KEY(session,delivery));
        DROP INDEX IF EXISTS encounter_delivery_active;
        CREATE UNIQUE INDEX encounter_delivery_active ON encounter_deliveries(session)
        WHERE phase IN ('dispatching','submitted','uncertain','queued');",
        )
        .map_err(failure)
}
/// The active phases: at most one of these per session may exist, so a queued
/// wait holds the same single delivery slot a dispatch does.
const ACTIVE_PHASES: &str = "('dispatching','submitted','uncertain','queued')";
fn row_delivery(
    session: &ResourceRef,
    delivery: &str,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<EncounterDelivery> {
    let (sender, request, phase, first_cursor, terminal_cursor, detail) = (
        row.get::<_, String>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
        row.get::<_, u64>(3)?,
        row.get::<_, Option<u64>>(4)?,
        row.get::<_, Option<String>>(5)?,
    );
    Ok(EncounterDelivery {
        agent_session: session.clone(),
        delivery_ref: ResourceRef::parse(delivery).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        sender: ResourceRef::parse(sender).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        request: serde_json::from_str(&request).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?,
        phase,
        first_cursor,
        terminal_cursor,
        detail,
    })
}
fn get(
    connection: &Connection,
    session: &ResourceRef,
    delivery: &ResourceRef,
) -> Result<Option<EncounterDelivery>> {
    connection
        .query_row(
            "SELECT sender,request,phase,first_cursor,terminal_cursor,detail FROM encounter_deliveries WHERE session=?1 AND delivery=?2",
            params![session.as_str(),delivery.as_str()],
            |row| row_delivery(session, delivery.as_str(), row),
        )
        .optional()
        .map_err(failure)
}
impl EncounterStore {
    /// Persist an intent *before* transport effects, under SQLite's writer lock.
    /// Restart/concurrent delivery can never interpret it as an unsent request.
    /// An uncertain delivery must be reconciled, never automatically resent.
    pub fn reserve_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        sender: &ResourceRef,
        request: &Value,
    ) -> Result<DeliveryReservation> {
        self.reserve_with_phase(session, delivery, sender, request, "dispatching")
    }
    /// Durable wait for a recipient whose resident is not currently live. The
    /// full sender-side admission has already run; this only records that the
    /// bounded addressed turn now waits for the recipient's next ready turn
    /// boundary. A queued row holds the session's single active delivery slot
    /// exactly as a dispatch does, survives restarts, and is resolved through
    /// the ordinary phase lifecycle once it is claimed for dispatch.
    pub fn queue_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        sender: &ResourceRef,
        request: &Value,
    ) -> Result<DeliveryReservation> {
        self.reserve_with_phase(session, delivery, sender, request, "queued")
    }
    fn reserve_with_phase(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        sender: &ResourceRef,
        request: &Value,
        phase: &str,
    ) -> Result<DeliveryReservation> {
        if !matches!(phase, "dispatching" | "queued") {
            return Err(failure(
                "A delivery is reserved only as dispatching or queued",
            ));
        }
        validate(session)?;
        ResourceRef::parse(delivery.as_str())?;
        ResourceRef::parse(sender.as_str())?;
        let body = serde_json::to_string(request).map_err(failure)?;
        if body.len() > 1024 * 1024 {
            return Err(failure("Machine delivery exceeds the 1 MiB limit"));
        }
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        if let Some(held) = get(&tx, session, delivery)? {
            if held.sender != *sender || held.request != *request {
                return Err(AikitError::new("encounter.delivery_conflict", "A delivery identity is already bound to another sender or request; no effect performed"));
            }
            return Ok(DeliveryReservation {
                fresh: false,
                delivery: held,
            });
        }
        if release_recovery(&tx, session)?.is_some() {
            return Err(AikitError::new(
                "encounter.native_release_uncertain",
                "Selected body cleanup is unresolved; no new machine-delivery admission",
            ));
        }
        let pending: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM encounter_deliveries WHERE session=?1 AND phase IN {ACTIVE_PHASES})"), [session.as_str()], |r|r.get(0)).map_err(failure)?;
        if pending {
            return Err(AikitError::new("encounter.delivery_pending", "This session has a queued, submitted or uncertain machine delivery; resolve or drain it before another effect"));
        }
        let event = stamp_observed_at(
            json!({"kind":"agent-message","sender":sender,"delivery_ref":delivery,"request":request,"standing":"machine-request-not-human-authorship"}),
        );
        tx.execute(
            "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
            params![session.as_str(), event.to_string()],
        )
        .map_err(failure)?;
        let cursor = tx.last_insert_rowid() as u64;
        tx.execute(&format!("INSERT INTO encounter_deliveries(session,delivery,sender,request,phase,first_cursor) VALUES(?1,?2,?3,?4,'{phase}',?5)"), params![session.as_str(),delivery.as_str(),sender.as_str(),body,cursor]).map_err(failure)?;
        let held =
            get(&tx, session, delivery)?.ok_or_else(|| failure("Reserved delivery disappeared"))?;
        tx.commit().map_err(failure)?;
        Ok(DeliveryReservation {
            fresh: true,
            delivery: held,
        })
    }
    /// Queued deliveries for a session, oldest first. These are admitted
    /// machine requests waiting for the recipient resident's next ready turn
    /// boundary; they are transport state, never human authorship.
    pub fn queued_deliveries(&self, session: &ResourceRef) -> Result<Vec<EncounterDelivery>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let mut statement = connection
            .prepare(
                "SELECT sender,request,phase,first_cursor,terminal_cursor,detail,delivery FROM encounter_deliveries WHERE session=?1 AND phase='queued' ORDER BY first_cursor ASC",
            )
            .map_err(failure)?;
        let rows = statement
            .query_map([session.as_str()], |row| {
                let delivery: String = row.get(6)?;
                row_delivery(session, &delivery, row)
            })
            .map_err(failure)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(failure)
    }
    /// Claim one queued delivery for dispatch at a ready resident turn
    /// boundary: queued → dispatching under the writer lock, recording the
    /// connection generation so provider events attribute to this delivery
    /// exactly as they do for a live send. From here the row resolves through
    /// the ordinary lifecycle (ack, reconcile, provider-journal finish).
    pub fn dispatch_queued_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        connection_generation: &str,
    ) -> Result<EncounterDelivery> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let changed = tx.execute(
            "UPDATE encounter_deliveries SET phase='dispatching',detail=NULL,request=json_set(request,'$.connection_generation',?3) WHERE session=?1 AND delivery=?2 AND phase='queued'",
            params![session.as_str(), delivery.as_str(), connection_generation],
        ).map_err(failure)?;
        if changed != 1 {
            return Err(AikitError::new(
                "encounter.delivery_changed",
                "The queued delivery is no longer waiting; reread it before dispatch",
            ));
        }
        tx.execute("INSERT INTO encounter_events(session,event) VALUES(?1,?2)",params![session.as_str(),stamp_observed_at(json!({"kind":"queued-delivery-dispatched","delivery_ref":delivery,"connection_generation":connection_generation})).to_string()]).map_err(failure)?;
        let held =
            get(&tx, session, delivery)?.ok_or_else(|| failure("Queued delivery disappeared"))?;
        tx.commit().map_err(failure)?;
        Ok(held)
    }
    /// Terminal refusal of a queued delivery at drain: a recipient-side
    /// admission or preparation check failed after the queue, so the message
    /// must never be delivered. The refusal is journaled, the row becomes
    /// failed (outside reconcile's reach), and the session's single active
    /// slot is released.
    pub fn refuse_queued_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        code: &str,
        reason: &str,
    ) -> Result<EncounterDelivery> {
        validate(session)?;
        let reason: String = reason.chars().take(512).collect();
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let existing =
            get(&tx, session, delivery)?.ok_or_else(|| failure("No such queued delivery"))?;
        if existing.phase != "queued" {
            return Err(AikitError::new(
                "encounter.delivery_changed",
                "The queued delivery is no longer waiting; reread it",
            ));
        }
        tx.execute("INSERT INTO encounter_events(session,event) VALUES(?1,?2)",params![session.as_str(),stamp_observed_at(json!({"kind":"queued-delivery-refused","delivery_ref":delivery,"code":code,"reason":reason,"standing":"admission-refused-at-drain-never-delivered"})).to_string()]).map_err(failure)?;
        let cursor = tx.last_insert_rowid() as u64;
        tx.execute("UPDATE encounter_deliveries SET phase='failed',terminal_cursor=?3,detail=?4 WHERE session=?1 AND delivery=?2 AND phase='queued'", params![session.as_str(),delivery.as_str(),cursor,format!("refused at drain: {code}")]).map_err(failure)?;
        let held =
            get(&tx, session, delivery)?.ok_or_else(|| failure("Queued delivery disappeared"))?;
        tx.commit().map_err(failure)?;
        Ok(held)
    }
    pub fn delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
    ) -> Result<Option<EncounterDelivery>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        get(&connection, session, delivery)
    }
    /// A positive transport ACK is not a model response. A negative or lost ACK
    /// is uncertain, not safe-to-retry; the provider may already have acted.
    pub fn delivery_ack(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        sent: bool,
        detail: Option<&str>,
    ) -> Result<EncounterDelivery> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        connection.execute("UPDATE encounter_deliveries SET phase=?3,detail=?4 WHERE session=?1 AND delivery=?2 AND phase='dispatching'",params![session.as_str(),delivery.as_str(),if sent {"submitted"} else {"uncertain"},detail]).map_err(failure)?;
        get(&connection, session, delivery)?.ok_or_else(|| failure("No such delivery"))
    }
    /// An owner-observed release is retained in the existing event journal.
    /// Caller labels and an absent resident cannot substitute for this receipt.
    pub fn native_release_receipt(
        &self,
        session: &ResourceRef,
        native: &str,
        generation: &str,
    ) -> Result<Option<Value>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        release_receipt(&connection, session, native, generation)
    }

    /// Reserve exact idle-body cleanup before the process effect. The same
    /// SQLite writer transaction excludes new machine-delivery admission.
    pub fn reserve_native_release(
        &self,
        session: &ResourceRef,
        native: &str,
        generation: &str,
    ) -> Result<Value> {
        validate(session)?;
        if native.trim().is_empty() || generation.trim().is_empty() {
            return Err(AikitError::new(
                "encounter.native_release_basis",
                "Native session and generation are required",
            ));
        }
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        if let Some(receipt) = release_receipt(&tx, session, native, generation)? {
            return Ok(receipt);
        }
        let binding: Option<String> = tx.query_row(
            "SELECT event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='binding' ORDER BY cursor DESC LIMIT 1",
            [session.as_str()], |row| row.get(0),
        ).optional().map_err(failure)?;
        let binding: Value = binding
            .map(|body| serde_json::from_str(&body).map_err(failure))
            .transpose()?
            .ok_or_else(|| {
                AikitError::new(
                    "encounter.native_release_basis",
                    "No actual retained native binding exists",
                )
            })?;
        if binding["native_session_id"].as_str() != Some(native)
            || binding["connection_generation"].as_str() != Some(generation)
        {
            return Err(AikitError::new(
                "encounter.native_release_basis",
                "The exact current native binding changed or has no generation",
            ));
        }
        let pending: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM encounter_deliveries WHERE session=?1 AND phase IN {ACTIVE_PHASES})"), [session.as_str()], |row| row.get(0)).map_err(failure)?;
        if pending {
            return Err(AikitError::new(
                "encounter.delivery_pending",
                "Queued, submitted or uncertain delivery prevents idle-body release",
            ));
        }
        if release_recovery(&tx, session)?.is_some() {
            return Err(AikitError::new(
                "encounter.native_release_uncertain",
                "A previous native release has no confirmed cleanup",
            ));
        }
        let event = stamp_observed_at(
            json!({"kind":"native-release-requested","native_session_id":native,"connection_generation":generation,"owner_pid":std::process::id(),"prior_native_binding":binding,"turn_replayed":false}),
        );
        tx.execute(
            "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
            params![session.as_str(), event.to_string()],
        )
        .map_err(failure)?;
        let cursor = tx.last_insert_rowid() as u64;
        tx.commit().map_err(failure)?;
        Ok(json!({"state":"Releasing","request_cursor":cursor,"request":event}))
    }

    /// Retain the cleanup result of the exact owned host. This is process
    /// cleanup, never model Return, effect verification or native resumption.
    pub fn finish_native_release(
        &self,
        session: &ResourceRef,
        native: &str,
        generation: &str,
        cleanup_confirmed: bool,
        process_status: Option<String>,
        cleanup_error: Option<String>,
    ) -> Result<Value> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        if let Some(receipt) = release_receipt(&tx, session, native, generation)? {
            return Ok(receipt);
        }
        let requested: Option<u64> = tx.query_row("SELECT cursor FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='native-release-requested' AND json_extract(event,'$.native_session_id')=?2 AND json_extract(event,'$.connection_generation')=?3 ORDER BY cursor DESC LIMIT 1",params![session.as_str(),native,generation],|row|row.get(0)).optional().map_err(failure)?;
        let request_cursor = requested.ok_or_else(|| {
            AikitError::new(
                "encounter.native_release_basis",
                "No exact release intent is retained",
            )
        })?;
        let event = stamp_observed_at(
            json!({"kind":"native-release-completed","agent_session":session,"native_session_id":native,"connection_generation":generation,"request_cursor":request_cursor,"cleanup_confirmed":cleanup_confirmed,"process_status":process_status,"cleanup_error":cleanup_error,"native_resume":false,"inference_observed":false,"turn_replayed":false}),
        );
        tx.execute(
            "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
            params![session.as_str(), event.to_string()],
        )
        .map_err(failure)?;
        let terminal_cursor = tx.last_insert_rowid() as u64;
        tx.commit().map_err(failure)?;
        Ok(
            json!({"state":if cleanup_confirmed {"Released"}else{"CleanupUncertain"},"request_cursor":request_cursor,"terminal_cursor":terminal_cursor,"receipt":event}),
        )
    }

    /// Correlate a selected predecessor with actual owner cleanup and its exact
    /// generation-bound startup basis. Old records are read, never rewritten.
    pub fn released_predecessor_basis(
        &self,
        session: &ResourceRef,
        native: &str,
        generation: &str,
        cleanup_cursor: u64,
    ) -> Result<Value> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let pending: bool = connection.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM encounter_deliveries WHERE session=?1 AND phase IN {ACTIVE_PHASES})"),
            [session.as_str()], |row| row.get(0),
        ).map_err(failure)?;
        if pending {
            return Err(AikitError::new(
                "encounter.delivery_pending",
                "Selected queued, active or uncertain delivery must settle before replacement",
            ));
        }
        let (binding_cursor, binding): (u64, String) = connection.query_row(
            "SELECT cursor,event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='binding' AND json_extract(event,'$.native_session_id')=?2 AND json_extract(event,'$.connection_generation')=?3 ORDER BY cursor DESC LIMIT 1",
            params![session.as_str(),native,generation], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional().map_err(failure)?.ok_or_else(|| AikitError::new("encounter.released_predecessor_absent", "Exact prior native binding is absent"))?;
        if cleanup_cursor <= binding_cursor {
            return Err(AikitError::new(
                "encounter.released_predecessor_changed",
                "Cleanup must follow the exact prior binding",
            ));
        }
        let cleanup: String = connection
            .query_row(
                "SELECT event FROM encounter_events WHERE session=?1 AND cursor=?2",
                params![session.as_str(), cleanup_cursor],
                |row| row.get(0),
            )
            .optional()
            .map_err(failure)?
            .ok_or_else(|| {
                AikitError::new(
                    "encounter.released_predecessor_absent",
                    "Selected cleanup event is absent",
                )
            })?;
        let binding: Value = serde_json::from_str(&binding).map_err(failure)?;
        let cleanup: Value = serde_json::from_str(&cleanup).map_err(failure)?;
        let released = cleanup["kind"] == "native-release-completed"
            && cleanup["native_session_id"].as_str() == Some(native)
            && cleanup["connection_generation"].as_str() == Some(generation)
            && cleanup["cleanup_confirmed"] == true;
        // EA4 supports full explicit Shutdown. Its per-session receipt lacks a
        // generation field, so correlate it with the selected binding and require
        // that no newer binding existed before that actual cleanup event.
        let shutdown = cleanup["kind"] == "owner-shutdown-completed"
            && cleanup["receipt"]["agent_session"] == json!(session)
            && cleanup["receipt"]["native_session_id"].as_str() == Some(native)
            && cleanup["receipt"]["process_stopped"] == true;
        let intervening: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM encounter_events WHERE session=?1 AND cursor>?2 AND cursor<?3 AND json_extract(event,'$.kind')='binding')",
            params![session.as_str(),binding_cursor,cleanup_cursor], |row| row.get(0),
        ).map_err(failure)?;
        if (!released && !shutdown) || intervening {
            return Err(AikitError::new(
                "encounter.released_predecessor_changed",
                "Actual cleanup is unconfirmed or another binding intervened",
            ));
        }
        let count: u64 = connection.query_row(
            "SELECT count(*) FROM encounter_events WHERE session=?1 AND cursor<?2 AND json_extract(event,'$.kind')='native-open-reserved' AND json_extract(event,'$.connection_generation')=?3",
            params![session.as_str(),binding_cursor,generation], |row| row.get(0),
        ).map_err(failure)?;
        if count != 1 {
            return Err(AikitError::new(
                "encounter.released_predecessor_changed",
                "Exact startup reservation is missing or ambiguous",
            ));
        }
        let opening: String = connection.query_row(
            "SELECT event FROM encounter_events WHERE session=?1 AND cursor<?2 AND json_extract(event,'$.kind')='native-open-reserved' AND json_extract(event,'$.connection_generation')=?3",
            params![session.as_str(),binding_cursor,generation], |row| row.get(0),
        ).map_err(failure)?;
        let opening: Value = serde_json::from_str(&opening).map_err(failure)?;
        Ok(json!({"binding_cursor":binding_cursor,"binding":binding,
            "cleanup_cursor":cleanup_cursor,"cleanup":cleanup,"opening":opening}))
    }
    /// Last observed native binding supports explicit reconnect. It never infers
    /// a WorldBinding from the session, provider or filesystem location.
    pub fn last_native_binding(&self, session: &ResourceRef) -> Result<Option<Value>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let body: Option<String> = connection.query_row("SELECT event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='binding' ORDER BY cursor DESC LIMIT 1", [session.as_str()], |r|r.get(0)).optional().map_err(failure)?;
        body.map(|b| serde_json::from_str(&b).map_err(failure))
            .transpose()
    }
    /// Reduce only the latest native-open generation to an unresolved recovery
    /// state. Current-process ownership is never reconstructed from the journal:
    /// an unmatched reservation requires native evidence, while a refusal whose
    /// cleanup was not confirmed remains uncertain across owner restarts.
    pub fn native_open_recovery(&self, session: &ResourceRef) -> Result<Option<Value>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        if let Some(recovery) = release_recovery(&connection, session)? {
            return Ok(Some(recovery));
        }
        let reserved: Option<(u64, String)> = connection
            .query_row(
                "SELECT cursor,event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='native-open-reserved' ORDER BY cursor DESC LIMIT 1",
                [session.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failure)?;
        let Some((reserved_cursor, reserved_body)) = reserved else {
            return Ok(None);
        };
        let reserved: Value = serde_json::from_str(&reserved_body).map_err(failure)?;
        let Some(generation) = reserved["connection_generation"].as_str() else {
            return Ok(Some(serde_json::json!({
                "state":"RecoveryRequired",
                "error":"Latest native startup reservation has no generation identity; explicit native reconciliation is required",
                "opening":reserved,
                "terminal":null
            })));
        };
        let terminal_body: Option<String> = connection
            .query_row(
                "SELECT event FROM encounter_events WHERE session=?1 AND cursor>?2 AND json_extract(event,'$.connection_generation')=?3 AND json_extract(event,'$.kind') IN ('binding','native-open-reconciled','native-open-refused') ORDER BY cursor DESC LIMIT 1",
                params![session.as_str(), reserved_cursor, generation],
                |row| row.get(0),
            )
            .optional()
            .map_err(failure)?;
        let Some(terminal_body) = terminal_body else {
            return Ok(Some(serde_json::json!({
                "state":"RecoveryRequired",
                "error":"A prior native startup has no generation-bound terminal evidence; explicit native reconciliation is required",
                "opening":reserved,
                "terminal":null
            })));
        };
        let terminal: Value = serde_json::from_str(&terminal_body).map_err(failure)?;
        if terminal["kind"] == "native-open-refused"
            && terminal["cleanup_confirmed"].as_bool() != Some(true)
        {
            return Ok(Some(serde_json::json!({
                "state":"CleanupUncertain",
                "error":terminal["reason"].as_str().unwrap_or("Native startup cleanup was not confirmed"),
                "opening":reserved,
                "terminal":terminal
            })));
        }
        Ok(None)
    }
    /// Unresolved startup truth across every canonical session retained in the
    /// owner journal, including sessions no longer attached to a SessionSpace.
    pub fn native_open_recoveries(&self) -> Result<Vec<(ResourceRef, Value)>> {
        let sessions = {
            let connection = self.connection.lock().map_err(failure)?;
            let mut query = connection
                .prepare("SELECT DISTINCT session FROM encounter_events WHERE json_extract(event,'$.kind')='native-open-reserved' ORDER BY session")
                .map_err(failure)?;
            let rows = query
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(failure)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(failure)?;
            rows
        };
        let mut unresolved = Vec::new();
        for session in sessions {
            let session = ResourceRef::parse(session)?;
            if let Some(recovery) = self.native_open_recovery(&session)? {
                unresolved.push((session, recovery));
            }
        }
        Ok(unresolved)
    }
    /// Explicit recovery for an uncertain request requires operator-supplied
    /// native evidence and cannot manufacture a successful response or retry it.
    pub fn reconcile_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
        evidence: &ResourceRef,
        expected_phase: &str,
    ) -> Result<EncounterDelivery> {
        if !matches!(expected_phase, "dispatching" | "submitted" | "uncertain") {
            return Err(failure("Only an unresolved delivery can be reconciled"));
        }
        validate(session)?;
        ResourceRef::parse(evidence.as_str())?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let changed=tx.execute("UPDATE encounter_deliveries SET phase='reconciled-no-replay',detail=?4 WHERE session=?1 AND delivery=?2 AND phase=?3", params![session.as_str(),delivery.as_str(),expected_phase,evidence.as_str()]).map_err(failure)?;
        if changed != 1 {
            return Err(AikitError::new(
                "encounter.delivery_changed",
                "Reread the current delivery before reconciling",
            ));
        }
        tx.execute("INSERT INTO encounter_events(session,event) VALUES(?1,?2)",params![session.as_str(),stamp_observed_at(json!({"kind":"delivery-reconciled","delivery_ref":delivery,"evidence_ref":evidence,"standing":"operator-native-evidence-correlation-not-success"})).to_string()]).map_err(failure)?;
        let result = get(&tx, session, delivery)?.ok_or_else(|| failure("No such delivery"))?;
        tx.commit().map_err(failure)?;
        Ok(result)
    }
}
/// Attribution and terminal state are committed with the native provider event.
/// A reply can win the race with the transport ACK without being overwritten.
pub(super) fn attribute(
    connection: &Connection,
    session: &ResourceRef,
    event: &Value,
) -> Result<Value> {
    if event["kind"] != "provider" {
        return Ok(event.clone());
    }
    let delivery: Option<String> = connection.query_row("SELECT delivery FROM encounter_deliveries WHERE session=?1 AND phase IN ('dispatching','submitted','uncertain') AND json_extract(request,'$.connection_generation') IS ?2",params![session.as_str(),event.get("connection_generation").and_then(Value::as_str)],|r|r.get(0)).optional().map_err(failure)?;
    let mut event = event.clone();
    if let Some(delivery) = delivery {
        event["delivery_ref"] = json!(delivery);
    }
    Ok(event)
}
pub(super) fn finish(
    connection: &Connection,
    session: &ResourceRef,
    event: &Value,
    cursor: u64,
) -> Result<()> {
    let Some(delivery) = event["delivery_ref"].as_str() else {
        return Ok(());
    };
    let Some(stop) = event.pointer("/event/TurnEnded/stop") else {
        return Ok(());
    };
    let phase = if stop.get("Completed").is_some() {
        "returned"
    } else if stop == "Cancelled" {
        "cancelled"
    } else {
        "failed"
    };
    connection.execute("UPDATE encounter_deliveries SET phase=?3,terminal_cursor=?4,detail=?5 WHERE session=?1 AND delivery=?2 AND phase IN ('dispatching','submitted','uncertain')",params![session.as_str(),delivery,phase,cursor,stop.to_string()]).map_err(failure)?;
    Ok(())
}

fn release_receipt(
    connection: &Connection,
    session: &ResourceRef,
    native: &str,
    generation: &str,
) -> Result<Option<Value>> {
    let row: Option<(u64,String)> = connection.query_row("SELECT cursor,event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='native-release-completed' AND json_extract(event,'$.native_session_id')=?2 AND json_extract(event,'$.connection_generation')=?3 ORDER BY cursor DESC LIMIT 1",params![session.as_str(),native,generation],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(failure)?;
    row.map(|(cursor,body)| { let receipt: Value=serde_json::from_str(&body).map_err(failure)?; Ok(json!({"state":if receipt["cleanup_confirmed"]==true {"Released"}else{"CleanupUncertain"},"request_cursor":receipt["request_cursor"],"terminal_cursor":cursor,"receipt":receipt})) }).transpose()
}

fn release_recovery(connection: &Connection, session: &ResourceRef) -> Result<Option<Value>> {
    let row: Option<(u64,String)> = connection.query_row("SELECT cursor,event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='native-release-requested' ORDER BY cursor DESC LIMIT 1",[session.as_str()],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(failure)?;
    let Some((cursor, body)) = row else {
        return Ok(None);
    };
    let request: Value = serde_json::from_str(&body).map_err(failure)?;
    let terminal = release_receipt(
        connection,
        session,
        request["native_session_id"].as_str().unwrap_or_default(),
        request["connection_generation"]
            .as_str()
            .unwrap_or_default(),
    )?;
    if terminal.as_ref().is_some_and(|value| {
        value["receipt"]["cleanup_confirmed"] == true
            && value["request_cursor"].as_u64() == Some(cursor)
    }) {
        return Ok(None);
    }
    Ok(Some(
        json!({"state":if terminal.is_some(){"CleanupUncertain"}else{"RecoveryRequired"},"error":"Exact native release cleanup is not confirmed; no body ownership is reconstructed from retained history","opening":request,"terminal":terminal}),
    ))
}
