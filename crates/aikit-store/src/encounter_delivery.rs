//! Durable machine delivery in the existing encounter journal. This is transport
//! state, never human authorship, completed work, or a Factory Recognition.
use super::{failure, validate, EncounterStore};
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
        CREATE UNIQUE INDEX IF NOT EXISTS encounter_delivery_active ON encounter_deliveries(session)
        WHERE phase IN ('dispatching','submitted','uncertain');",
        )
        .map_err(failure)
}
fn get(
    connection: &Connection,
    session: &ResourceRef,
    delivery: &ResourceRef,
) -> Result<Option<EncounterDelivery>> {
    let row = connection.query_row(
        "SELECT sender,request,phase,first_cursor,terminal_cursor,detail FROM encounter_deliveries WHERE session=?1 AND delivery=?2",
        params![session.as_str(),delivery.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,u64>(3)?,row.get::<_,Option<u64>>(4)?,row.get::<_,Option<String>>(5)?)),
    ).optional().map_err(failure)?;
    row.map(
        |(sender, request, phase, first_cursor, terminal_cursor, detail)| {
            Ok(EncounterDelivery {
                agent_session: session.clone(),
                delivery_ref: delivery.clone(),
                sender: ResourceRef::parse(sender)?,
                request: serde_json::from_str(&request).map_err(failure)?,
                phase,
                first_cursor,
                terminal_cursor,
                detail,
            })
        },
    )
    .transpose()
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
        let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM encounter_deliveries WHERE session=?1 AND phase IN ('dispatching','submitted','uncertain'))", [session.as_str()], |r|r.get(0)).map_err(failure)?;
        if pending {
            return Err(AikitError::new("encounter.delivery_pending", "This session has a submitted or uncertain machine delivery; reconcile its actual result before another effect"));
        }
        let event = json!({"kind":"agent-message","sender":sender,"delivery_ref":delivery,"request":request,"standing":"machine-request-not-human-authorship"});
        tx.execute(
            "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
            params![session.as_str(), event.to_string()],
        )
        .map_err(failure)?;
        let cursor = tx.last_insert_rowid() as u64;
        tx.execute("INSERT INTO encounter_deliveries(session,delivery,sender,request,phase,first_cursor) VALUES(?1,?2,?3,?4,'dispatching',?5)", params![session.as_str(),delivery.as_str(),sender.as_str(),body,cursor]).map_err(failure)?;
        let held =
            get(&tx, session, delivery)?.ok_or_else(|| failure("Reserved delivery disappeared"))?;
        tx.commit().map_err(failure)?;
        Ok(DeliveryReservation {
            fresh: true,
            delivery: held,
        })
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
    /// Last observed native binding supports explicit reconnect. It never infers
    /// a WorldBinding from the session, provider or filesystem location.
    pub fn last_native_binding(&self, session: &ResourceRef) -> Result<Option<Value>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let body: Option<String> = connection.query_row("SELECT event FROM encounter_events WHERE session=?1 AND json_extract(event,'$.kind')='binding' ORDER BY cursor DESC LIMIT 1", [session.as_str()], |r|r.get(0)).optional().map_err(failure)?;
        body.map(|b| serde_json::from_str(&b).map_err(failure))
            .transpose()
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
        tx.execute("INSERT INTO encounter_events(session,event) VALUES(?1,?2)",params![session.as_str(),json!({"kind":"delivery-reconciled","delivery_ref":delivery,"evidence_ref":evidence,"standing":"operator-native-evidence-correlation-not-success"}).to_string()]).map_err(failure)?;
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
