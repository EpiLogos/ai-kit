//! Durable conversation requests in the encounter journal (O:I #558, PF2).
//!
//! One request binds an authored Flow entry to its recipients. Each recipient
//! has its own delivery (the existing per-session `encounter_deliveries` row),
//! its own dispatch standing, and its own inclusion of the returned reply in
//! the Flow. This is coordinated state, not a transaction across products:
//! every independently successful effect is recorded and the remaining step is
//! reconciled from here, so a dead process, a closed UI or a busy recipient
//! leaves a readable, resumable record. Nothing in it is human authorship, a
//! completed task, or proof that a recipient understood anything.
//!
//! The reply is reduced from the owner's own journal: the agent-message chunks
//! that carry the delivery's ref between its first and terminal cursor. No
//! provider runtime offers a turn or output id, so correlation is the
//! per-session serialization the delivery table already enforces, never the
//! newest assistant block.
use super::{failure, stamp_observed_at, validate, EncounterStore};
use crate::encounter::EncounterDelivery;
use aikit_core::{AikitError, ResourceRef, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A reply is retained whole up to this bound; beyond it the reading discloses
/// the continuation instead of truncating silently.
pub const REPLY_LIMIT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewConversationRecipient {
    pub participant_key: String,
    pub agent_session: ResourceRef,
    pub delivery_ref: ResourceRef,
    #[serde(default)]
    pub agent_ref: Option<String>,
    /// Where this recipient's session lives when it is not on this owner:
    /// `{kind:"ssh", target, cwd?, aikit?, workcell}`. Absent = this owner.
    #[serde(default)]
    pub route: Option<Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReplyReading {
    pub text: String,
    /// The delivery's turn ended as completed.
    pub complete: bool,
    pub bytes: usize,
    /// More output exists than the bound retains.
    pub truncated: bool,
    pub first_cursor: u64,
    pub last_cursor: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationRecipientReading {
    pub participant_key: String,
    pub agent_session: ResourceRef,
    pub delivery_ref: ResourceRef,
    pub agent_ref: Option<String>,
    /// The remote route this recipient is reached by, if not local.
    pub route: Option<Value>,
    /// unsent | sent | held | refused
    pub dispatch: String,
    pub dispatch_detail: Option<String>,
    pub delivery: Option<EncounterDelivery>,
    pub reply: Option<ReplyReading>,
    /// pending | included | conflict | failed | refused
    pub inclusion: String,
    pub inclusion_detail: Option<String>,
    pub entry_id: Option<String>,
    pub revision: Option<String>,
    pub attempts: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationReading {
    pub request_ref: ResourceRef,
    pub body: Value,
    /// Where the authored entry landed in the Flow once it was committed:
    /// `{entry_id, revision, document_revision}`. Absent until then.
    pub source: Option<Value>,
    pub recipients: Vec<ConversationRecipientReading>,
}
/// Work the owner's worker can do now, derived from durable state alone.
#[derive(Debug, Clone, PartialEq)]
pub enum ConversationWork {
    /// A remote recipient was asked; its own owner's delivery and reply are
    /// read until the turn ends, then snapshotted here before any inclusion.
    Poll {
        request: ResourceRef,
        participant: String,
    },
    /// Recipient never reached the dispatch boundary (or was held busy).
    Dispatch {
        request: ResourceRef,
        participant: String,
    },
    /// Recipient's turn returned and its reply is not yet in the Flow.
    Incorporate {
        request: ResourceRef,
        participant: String,
    },
}

pub(super) fn install(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_requests(
            request TEXT PRIMARY KEY, digest TEXT NOT NULL, body TEXT NOT NULL, created_ms INTEGER NOT NULL, source TEXT);
            CREATE TABLE IF NOT EXISTS conversation_recipients(
            request TEXT NOT NULL, participant TEXT NOT NULL, session TEXT NOT NULL, delivery TEXT NOT NULL, agent TEXT,
            dispatch TEXT NOT NULL DEFAULT 'unsent', dispatch_detail TEXT,
            inclusion TEXT NOT NULL DEFAULT 'pending', inclusion_detail TEXT, entry_id TEXT, revision TEXT,
            attempts INTEGER NOT NULL DEFAULT 0, route TEXT, remote TEXT,
            PRIMARY KEY(request,participant));
            CREATE INDEX IF NOT EXISTS conversation_recipient_delivery ON conversation_recipients(session,delivery);",
        )
        .map_err(failure)?;
    // Homes that recorded requests before remote routes existed gain the columns.
    for column in ["route TEXT", "remote TEXT"] {
        let _ = connection.execute(
            &format!("ALTER TABLE conversation_recipients ADD COLUMN {column}"),
            [],
        );
    }
    Ok(())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Reduce the reply for one delivery from the journal. Events are selected by
/// the delivery ref the owner stamped at append, so another delivery's output
/// on the same session can never be read as this reply.
fn reduce_reply(
    connection: &Connection,
    delivery: &EncounterDelivery,
) -> Result<Option<ReplyReading>> {
    let mut query = connection
        .prepare(
            "SELECT cursor, json_extract(event,'$.event.Signal.kind.text') FROM encounter_events
             WHERE session=?1 AND cursor>=?2 AND (?3 IS NULL OR cursor<=?3)
               AND json_extract(event,'$.delivery_ref')=?4
               AND json_extract(event,'$.event.Signal.kind.kind')='agent-message-chunk'
             ORDER BY cursor",
        )
        .map_err(failure)?;
    let rows = query
        .query_map(
            params![
                delivery.agent_session.as_str(),
                delivery.first_cursor,
                delivery.terminal_cursor,
                delivery.delivery_ref.as_str()
            ],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(failure)?;
    let mut text = String::new();
    let mut last = None;
    let mut bytes = 0usize;
    let mut truncated = false;
    for row in rows {
        let (cursor, piece) = row.map_err(failure)?;
        let piece = piece.unwrap_or_default();
        bytes += piece.len();
        last = Some(cursor);
        if truncated {
            continue;
        }
        if text.len() + piece.len() > REPLY_LIMIT_BYTES {
            let mut end = REPLY_LIMIT_BYTES - text.len();
            while !piece.is_char_boundary(end) {
                end -= 1;
            }
            text.push_str(&piece[..end]);
            truncated = true;
        } else {
            text.push_str(&piece);
        }
    }
    if last.is_none() && delivery.phase != "returned" {
        return Ok(None);
    }
    Ok(Some(ReplyReading {
        text,
        complete: delivery.phase == "returned",
        bytes,
        truncated,
        first_cursor: delivery.first_cursor,
        last_cursor: last,
    }))
}

fn delivery_of(
    connection: &Connection,
    session: &ResourceRef,
    delivery: &ResourceRef,
) -> Result<Option<EncounterDelivery>> {
    connection
        .query_row(
            "SELECT sender,request,phase,first_cursor,terminal_cursor,detail FROM encounter_deliveries WHERE session=?1 AND delivery=?2",
            params![session.as_str(), delivery.as_str()],
            |row| {
                let parse = |text: String| {
                    ResourceRef::parse(text).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                    })
                };
                Ok(EncounterDelivery {
                    agent_session: session.clone(),
                    delivery_ref: delivery.clone(),
                    sender: parse(row.get(0)?)?,
                    request: serde_json::from_str(&row.get::<_, String>(1)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
                    })?,
                    phase: row.get(2)?,
                    first_cursor: row.get(3)?,
                    terminal_cursor: row.get(4)?,
                    detail: row.get(5)?,
                })
            },
        )
        .optional()
        .map_err(failure)
}

fn read_request(
    connection: &Connection,
    request: &ResourceRef,
) -> Result<Option<ConversationReading>> {
    let held: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT body,source FROM conversation_requests WHERE request=?1",
            [request.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(failure)?;
    let Some((body, source)) = held else {
        return Ok(None);
    };
    let mut query = connection
        .prepare(
            "SELECT participant,session,delivery,agent,dispatch,dispatch_detail,inclusion,inclusion_detail,entry_id,revision,attempts,route,remote
             FROM conversation_recipients WHERE request=?1 ORDER BY rowid",
        )
        .map_err(failure)?;
    let rows = query
        .query_map([request.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, u32>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
            ))
        })
        .map_err(failure)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(failure)?;
    let mut recipients = Vec::new();
    for (
        participant,
        session,
        delivery,
        agent,
        dispatch,
        dispatch_detail,
        inclusion,
        inclusion_detail,
        entry_id,
        revision,
        attempts,
        route,
        remote,
    ) in rows
    {
        let session = ResourceRef::parse(session)?;
        let delivery = ResourceRef::parse(delivery)?;
        let route: Option<Value> = route
            .map(|r| serde_json::from_str(&r))
            .transpose()
            .map_err(failure)?;
        let remote: Option<Value> = remote
            .map(|r| serde_json::from_str(&r))
            .transpose()
            .map_err(failure)?;
        // A local recipient's delivery and reply are this owner's journal; a
        // remote recipient's are its own owner's, known here by the durable
        // snapshot this owner took of them.
        let (held, reply) = if route.is_some() {
            let delivery_row = remote
                .as_ref()
                .and_then(|r| r.get("delivery"))
                .and_then(|d| serde_json::from_value::<EncounterDelivery>(d.clone()).ok());
            let reply = remote
                .as_ref()
                .and_then(|r| r.get("reply"))
                .filter(|r| !r.is_null())
                .and_then(|r| serde_json::from_value::<ReplyReading>(r.clone()).ok());
            (delivery_row, reply)
        } else {
            let held = delivery_of(connection, &session, &delivery)?;
            let reply = match &held {
                Some(d) => reduce_reply(connection, d)?,
                None => None,
            };
            (held, reply)
        };
        recipients.push(ConversationRecipientReading {
            participant_key: participant,
            agent_session: session,
            delivery_ref: delivery,
            agent_ref: agent,
            route,
            dispatch,
            dispatch_detail,
            delivery: held,
            reply,
            inclusion,
            inclusion_detail,
            entry_id,
            revision,
            attempts,
        });
    }
    Ok(Some(ConversationReading {
        request_ref: request.clone(),
        body: serde_json::from_str(&body).map_err(failure)?,
        source: source
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(failure)?,
        recipients,
    }))
}

impl EncounterStore {
    /// Record a conversation request and its recipients before any effect. The
    /// request identity is idempotent: the same ref with the same digest is a
    /// replay; the same ref with another digest is refused and changes nothing.
    pub fn create_conversation(
        &self,
        request: &ResourceRef,
        digest: &str,
        body: &Value,
        recipients: &[NewConversationRecipient],
    ) -> Result<(bool, ConversationReading)> {
        if recipients.is_empty() || recipients.len() > 32 {
            return Err(failure("A conversation request needs 1–32 recipients"));
        }
        let mut sessions = std::collections::BTreeSet::new();
        let mut participants = std::collections::BTreeSet::new();
        for recipient in recipients {
            validate(&recipient.agent_session)?;
            if !sessions.insert(recipient.agent_session.clone())
                || !participants.insert(recipient.participant_key.clone())
            {
                return Err(AikitError::new(
                    "conversation.duplicate_recipient",
                    "One participant or session appears twice in this request; a recipient is scheduled once",
                ));
            }
        }
        let body_text = serde_json::to_string(body).map_err(failure)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let held: Option<(String, String)> = tx
            .query_row(
                "SELECT digest,body FROM conversation_requests WHERE request=?1",
                [request.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(failure)?;
        // Optional document basis participates in this existing transaction,
        // separately from the unchanged legacy digest. Concurrent replays may
        // not add, drop or substitute a pin under the same request identity.
        if let Some((_, retained)) = &held {
            let retained: Value = serde_json::from_str(retained).map_err(failure)?;
            let retained_pin = retained
                .pointer("/flow/document_id")
                .filter(|value| !value.is_null());
            let requested_pin = body
                .pointer("/flow/document_id")
                .filter(|value| !value.is_null());
            if retained_pin != requested_pin {
                return Err(AikitError::new("conversation.request_conflict", "This request identity is bound to a different document basis; no effect performed"));
            }
        }
        let fresh = match held {
            Some((existing, _)) if existing != digest => {
                return Err(AikitError::new(
                    "conversation.request_conflict",
                    "This request identity is already bound to a different entry, target or basis; no effect performed",
                ))
            }
            Some(_) => false,
            None => {
                tx.execute(
                    "INSERT INTO conversation_requests(request,digest,body,created_ms) VALUES(?1,?2,?3,?4)",
                    params![request.as_str(), digest, body_text, now_ms()],
                )
                .map_err(failure)?;
                for recipient in recipients {
                    tx.execute(
                        "INSERT INTO conversation_recipients(request,participant,session,delivery,agent,route) VALUES(?1,?2,?3,?4,?5,?6)",
                        params![
                            request.as_str(),
                            recipient.participant_key,
                            recipient.agent_session.as_str(),
                            recipient.delivery_ref.as_str(),
                            recipient.agent_ref,
                            recipient.route.as_ref().map(|r| r.to_string())
                        ],
                    )
                    .map_err(failure)?;
                }
                tx.execute(
                    "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
                    params![
                        recipients[0].agent_session.as_str(),
                        stamp_observed_at(json!({"kind":"conversation-request-recorded","request_ref":request,"standing":"coordination-record-not-authorship"})).to_string()
                    ],
                )
                .map_err(failure)?;
                true
            }
        };
        let reading =
            read_request(&tx, request)?.ok_or_else(|| failure("Recorded request disappeared"))?;
        tx.commit().map_err(failure)?;
        Ok((fresh, reading))
    }
    /// Record where the authored entry landed. Set once; the same entry
    /// recorded again is a no-op, a different one is refused.
    pub fn conversation_record_source(&self, request: &ResourceRef, source: &Value) -> Result<()> {
        let text = serde_json::to_string(source).map_err(failure)?;
        let connection = self.connection.lock().map_err(failure)?;
        let held: Option<Option<String>> = connection
            .query_row(
                "SELECT source FROM conversation_requests WHERE request=?1",
                [request.as_str()],
                |r| r.get(0),
            )
            .optional()
            .map_err(failure)?;
        match held {
            None => Err(failure("No such conversation request")),
            Some(Some(existing)) => {
                let existing: Value = serde_json::from_str(&existing).map_err(failure)?;
                if existing["entry_id"] == source["entry_id"] {
                    Ok(())
                } else {
                    Err(AikitError::new(
                        "conversation.source_conflict",
                        "This request is already bound to another Flow entry",
                    ))
                }
            }
            Some(None) => {
                connection
                    .execute("UPDATE conversation_requests SET source=?2 WHERE request=?1 AND source IS NULL", params![request.as_str(), text])
                    .map_err(failure)?;
                Ok(())
            }
        }
    }
    pub fn conversation(&self, request: &ResourceRef) -> Result<Option<ConversationReading>> {
        let connection = self.connection.lock().map_err(failure)?;
        read_request(&connection, request)
    }
    /// Requests bound to one Flow entry's source (newest first, bounded).
    pub fn conversations_for_flow(
        &self,
        flow_ref: &str,
        limit: usize,
    ) -> Result<Vec<ConversationReading>> {
        let refs: Vec<String> = {
            let connection = self.connection.lock().map_err(failure)?;
            let mut query = connection
                .prepare(
                    "SELECT request FROM conversation_requests WHERE json_extract(body,'$.flow.location.ref')=?1 ORDER BY created_ms DESC, rowid DESC LIMIT ?2",
                )
                .map_err(failure)?;
            let rows = query
                .query_map(params![flow_ref, limit.min(200) as i64], |r| {
                    r.get::<_, String>(0)
                })
                .map_err(failure)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(failure)?;
            rows
        };
        let connection = self.connection.lock().map_err(failure)?;
        refs.into_iter()
            .filter_map(|r| ResourceRef::parse(r).ok())
            .map(|r| read_request(&connection, &r))
            .filter_map(|r| r.transpose())
            .collect()
    }
    pub fn conversation_set_dispatch(
        &self,
        request: &ResourceRef,
        participant: &str,
        dispatch: &str,
        detail: Option<&str>,
    ) -> Result<()> {
        if !matches!(dispatch, "unsent" | "sent" | "held" | "refused") {
            return Err(failure("Unknown dispatch standing"));
        }
        let connection = self.connection.lock().map_err(failure)?;
        connection
            .execute(
                "UPDATE conversation_recipients SET dispatch=?3,dispatch_detail=?4 WHERE request=?1 AND participant=?2 AND dispatch NOT IN ('sent','refused')",
                params![request.as_str(), participant, dispatch, detail],
            )
            .map_err(failure)?;
        Ok(())
    }
    /// Record where inclusion of a recipient's reply stands. `included` is
    /// terminal and keeps the Flow entry it became; every other standing can be
    /// retried by the worker.
    pub fn conversation_record_inclusion(
        &self,
        request: &ResourceRef,
        participant: &str,
        inclusion: &str,
        entry_id: Option<&str>,
        revision: Option<&str>,
        detail: Option<&str>,
    ) -> Result<()> {
        if !matches!(
            inclusion,
            "pending" | "included" | "conflict" | "failed" | "refused"
        ) {
            return Err(failure("Unknown inclusion standing"));
        }
        let connection = self.connection.lock().map_err(failure)?;
        connection
            .execute(
                "UPDATE conversation_recipients SET inclusion=?3,entry_id=COALESCE(?4,entry_id),revision=COALESCE(?5,revision),inclusion_detail=?6,attempts=attempts+1
                 WHERE request=?1 AND participant=?2 AND inclusion<>'included'",
                params![request.as_str(), participant, inclusion, entry_id, revision, detail],
            )
            .map_err(failure)?;
        Ok(())
    }
    /// Record the durable snapshot of a remote recipient's delivery and reply,
    /// taken from its own owner. Once the turn ended the snapshot is final:
    /// inclusion works from it, never from a second read of the remote.
    pub fn conversation_record_remote(
        &self,
        request: &ResourceRef,
        participant: &str,
        remote: &Value,
    ) -> Result<()> {
        let connection = self.connection.lock().map_err(failure)?;
        connection
            .execute(
                "UPDATE conversation_recipients SET remote=?3 WHERE request=?1 AND participant=?2 AND route IS NOT NULL
                   AND (remote IS NULL OR json_extract(remote,'$.delivery.phase') NOT IN ('returned','failed','cancelled','reconciled-no-replay'))",
                params![request.as_str(), participant, remote.to_string()],
            )
            .map_err(failure)?;
        Ok(())
    }
    /// What the owner's worker should do now, from durable state only.
    pub fn conversation_work(&self, limit: usize) -> Result<Vec<ConversationWork>> {
        let connection = self.connection.lock().map_err(failure)?;
        let mut query = connection
            .prepare(
                "SELECT r.request, r.participant,
                   CASE
                     WHEN r.dispatch IN ('unsent','held') THEN 'dispatch'
                     WHEN r.route IS NULL AND r.inclusion IN ('pending','failed','conflict') AND d.phase='returned' THEN 'incorporate'
                     WHEN r.route IS NOT NULL AND r.dispatch='sent' AND r.inclusion IN ('pending','failed','conflict')
                          AND json_extract(r.remote,'$.delivery.phase')='returned' THEN 'incorporate'
                     WHEN r.route IS NOT NULL AND r.dispatch='sent' AND r.inclusion IN ('pending','failed','conflict')
                          AND (r.remote IS NULL OR json_extract(r.remote,'$.delivery.phase') NOT IN ('returned','failed','cancelled','reconciled-no-replay')) THEN 'poll'
                   END AS work
                 FROM conversation_recipients r
                 LEFT JOIN encounter_deliveries d ON d.session=r.session AND d.delivery=r.delivery AND r.route IS NULL
                 ORDER BY r.rowid",
            )
            .map_err(failure)?;
        let rows = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(failure)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(failure)?;
        rows.into_iter()
            .filter_map(|(request, participant, work)| {
                let request = ResourceRef::parse(request).ok()?;
                match work.as_deref() {
                    Some("dispatch") => Some(ConversationWork::Dispatch {
                        request,
                        participant,
                    }),
                    Some("incorporate") => Some(ConversationWork::Incorporate {
                        request,
                        participant,
                    }),
                    Some("poll") => Some(ConversationWork::Poll {
                        request,
                        participant,
                    }),
                    _ => None,
                }
            })
            .take(limit)
            .map(Ok)
            .collect()
    }
    /// The conversation recipient a local delivery belongs to, if it belongs to
    /// one: the request and participant it was made for.
    pub fn conversation_recipient_for_delivery(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
    ) -> Result<Option<(ResourceRef, String)>> {
        let connection = self.connection.lock().map_err(failure)?;
        let held: Option<(String, String)> = connection
            .query_row(
                "SELECT request,participant FROM conversation_recipients WHERE session=?1 AND delivery=?2 AND route IS NULL",
                params![session.as_str(), delivery.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(failure)?;
        held.map(|(request, participant)| Ok((ResourceRef::parse(request)?, participant)))
            .transpose()
    }
    /// Sessions that hold a queued conversation delivery: a recipient asked while
    /// its session was busy with a turn of its own (a composer turn, another
    /// delivery) waits durably here for the turn boundary. The owner's worker
    /// drains these when that turn ends; it does not wait for a session open.
    pub fn conversation_queued_sessions(&self) -> Result<Vec<ResourceRef>> {
        let connection = self.connection.lock().map_err(failure)?;
        let mut query = connection
            .prepare(
                "SELECT DISTINCT d.session FROM encounter_deliveries d
                 JOIN conversation_recipients r ON r.session=d.session AND r.delivery=d.delivery AND r.route IS NULL
                 WHERE d.phase='queued' AND r.dispatch='sent' AND r.inclusion NOT IN ('included','refused') ORDER BY d.session",
            )
            .map_err(failure)?;
        let rows = query
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(failure)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(failure)?;
        rows.into_iter().map(ResourceRef::parse).collect()
    }
    /// A recipient that was asked (its delivery is waiting queued) is refused
    /// before it runs: the standing is `refused`, with the reason. Only a
    /// dispatch that has not started a turn can be refused this way.
    pub fn conversation_refuse_queued(
        &self,
        request: &ResourceRef,
        participant: &str,
        detail: &str,
    ) -> Result<()> {
        let connection = self.connection.lock().map_err(failure)?;
        connection
            .execute(
                "UPDATE conversation_recipients SET dispatch='refused',dispatch_detail=?3,inclusion='refused',inclusion_detail=?3
                 WHERE request=?1 AND participant=?2 AND inclusion<>'included' AND dispatch IN ('sent','held','unsent')",
                params![request.as_str(), participant, detail],
            )
            .map_err(failure)?;
        Ok(())
    }
    /// Reconcile, at owner start, every local conversation delivery that was
    /// sent (or was about to be) but whose provider turn has no live
    /// continuation in this owner: the resident that carried it died with the
    /// previous owner. The journal decides. If it holds the turn's terminal event
    /// the delivery is finished from it (the reply is then included by the
    /// ordinary worker). If it does not, the turn's outcome is unknown: the
    /// delivery becomes `reconciled-no-replay` — read as `uncertain` — with the
    /// exact source continuation named, which also releases the session's single
    /// delivery slot. The turn is never dispatched again. `live` holds the
    /// connection generations this owner actually carries.
    pub fn conversation_reconcile_lost(
        &self,
        live: &std::collections::BTreeSet<String>,
    ) -> Result<Vec<Value>> {
        let mut connection = self.connection.lock().map_err(failure)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let candidates: Vec<(String, String, String, u64, Option<String>)> = {
            let mut query = tx
                .prepare(
                    "SELECT d.session,d.delivery,d.phase,d.first_cursor,json_extract(d.request,'$.connection_generation')
                     FROM encounter_deliveries d
                     JOIN conversation_recipients r ON r.session=d.session AND r.delivery=d.delivery AND r.route IS NULL
                     WHERE d.phase IN ('dispatching','submitted','uncertain') ORDER BY d.first_cursor",
                )
                .map_err(failure)?;
            let rows = query
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .map_err(failure)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(failure)?;
            rows
        };
        let mut reconciled = Vec::new();
        for (session, delivery, phase, first_cursor, generation) in candidates {
            if generation.as_ref().is_some_and(|g| live.contains(g)) {
                continue;
            }
            // Terminal evidence for this very turn: an ended turn on the same
            // connection generation after the delivery was reserved.
            let terminal: Option<(u64, String)> = tx
                .query_row(
                    "SELECT cursor, json_extract(event,'$.event.TurnEnded') FROM encounter_events
                     WHERE session=?1 AND cursor>?2 AND json_extract(event,'$.event.TurnEnded') IS NOT NULL
                       AND (json_extract(event,'$.delivery_ref')=?3
                            OR (json_extract(event,'$.delivery_ref') IS NULL AND ?4 IS NOT NULL
                                AND json_extract(event,'$.connection_generation')=?4))
                     ORDER BY cursor LIMIT 1",
                    params![session, first_cursor, delivery, generation],
                    |r| Ok((r.get(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(failure)?;
            if let Some((cursor, ended_turn)) = terminal {
                let stop = serde_json::from_str::<Value>(&ended_turn)
                    .ok()
                    .and_then(|turn| turn.get("stop").cloned())
                    .unwrap_or(Value::Null);
                let ended = if stop.get("Completed").is_some() {
                    "returned"
                } else if stop == "Cancelled" {
                    "cancelled"
                } else {
                    "failed"
                };
                tx.execute(
                    "UPDATE encounter_deliveries SET phase=?3,terminal_cursor=?4,detail=?5 WHERE session=?1 AND delivery=?2 AND phase IN ('dispatching','submitted','uncertain')",
                    params![session, delivery, ended, cursor, stop.to_string()],
                )
                .map_err(failure)?;
                reconciled.push(json!({"delivery_ref": delivery, "outcome": ended, "terminal_cursor": cursor, "source": "owner-journal-terminal-event"}));
                continue;
            }
            let last: Option<u64> = tx
                .query_row(
                    "SELECT MAX(cursor) FROM encounter_events WHERE session=?1 AND cursor>=?2 AND (json_extract(event,'$.delivery_ref')=?3
                       OR (json_extract(event,'$.delivery_ref') IS NULL AND ?4 IS NOT NULL AND json_extract(event,'$.connection_generation')=?4))",
                    params![session, first_cursor, delivery, generation],
                    |r| r.get(0),
                )
                .map_err(failure)?;
            // The provider's own session id, as its events for this turn (or this
            // connection generation) named it; the open's binding when none did.
            let native: Option<String> = tx
                .query_row(
                    "SELECT json_extract(event,'$.event.Signal.native_session_id') FROM encounter_events
                     WHERE session=?1 AND cursor>=?2 AND json_extract(event,'$.event.Signal.native_session_id') IS NOT NULL
                       AND (json_extract(event,'$.delivery_ref')=?3
                            OR (json_extract(event,'$.delivery_ref') IS NULL AND ?4 IS NOT NULL
                                AND json_extract(event,'$.connection_generation')=?4))
                     ORDER BY cursor DESC LIMIT 1",
                    params![session, first_cursor, delivery, generation],
                    |r| r.get(0),
                )
                .optional()
                .map_err(failure)?;
            let native = match native {
                Some(native) => Some(native),
                None => tx
                    .query_row(
                        "SELECT json_extract(event,'$.native_session_id') FROM encounter_events
                         WHERE session=?1 AND json_extract(event,'$.kind')='binding' AND cursor<=?2
                           AND (?3 IS NULL OR json_extract(event,'$.connection_generation') IS ?3)
                         ORDER BY cursor DESC LIMIT 1",
                        params![session, first_cursor, generation],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(failure)?
                    .flatten(),
            };
            let continuation = format!(
                "source continuation: session {session}, connection generation {}, native session {}, journal cursors {first_cursor}..{}",
                generation.as_deref().unwrap_or("unrecorded"),
                native.as_deref().unwrap_or("unrecorded"),
                last.map_or("none".to_owned(), |c| c.to_string()),
            );
            let detail = format!(
                "owner restarted with no live provider continuation for this delivery (was {phase}) and the journal holds no terminal event for its turn; the outcome is unknown, the turn was not replayed. {continuation}"
            );
            tx.execute(
                "UPDATE encounter_deliveries SET phase='reconciled-no-replay',detail=?3 WHERE session=?1 AND delivery=?2 AND phase IN ('dispatching','submitted','uncertain')",
                params![session, delivery, detail],
            )
            .map_err(failure)?;
            tx.execute(
                "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
                params![
                    session,
                    stamp_observed_at(json!({
                        "kind":"delivery-continuation-lost","delivery_ref":delivery,"was_phase":phase,
                        "connection_generation":generation,"native_session_id":native,
                        "first_cursor":first_cursor,"last_cursor":last,
                        "standing":"owner-journal-reconciliation: no terminal event; not success, not replayed"
                    }))
                    .to_string()
                ],
            )
            .map_err(failure)?;
            reconciled.push(json!({"delivery_ref": delivery, "outcome": "uncertain", "detail": detail, "source": "owner-journal-no-terminal-event"}));
        }
        tx.commit().map_err(failure)?;
        Ok(reconciled)
    }
    /// The reply a delivery produced, reduced from the owner journal.
    pub fn delivery_reply(
        &self,
        session: &ResourceRef,
        delivery: &ResourceRef,
    ) -> Result<Option<ReplyReading>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        match delivery_of(&connection, session, delivery)? {
            Some(held) => reduce_reply(&connection, &held),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AikitHome;

    fn chunk(generation: &str, text: &str) -> Value {
        json!({"kind":"provider","connection_generation":generation,
               "event":{"Signal":{"sequence":1,"native_session_id":"n","kind":{"kind":"agent-message-chunk","text":text}}}})
    }
    fn turn_end(generation: &str) -> Value {
        json!({"kind":"provider","connection_generation":generation,"event":{"TurnEnded":{"stop":{"Completed":"EndTurn"}}}})
    }
    fn r(s: &str) -> ResourceRef {
        ResourceRef::parse(s).unwrap()
    }
    fn dispatch(store: &EncounterStore, session: &ResourceRef, delivery: &str, generation: &str) {
        store
            .reserve_delivery(
                session,
                &r(delivery),
                &r("human:ann"),
                &json!({"connection_generation": generation}),
            )
            .unwrap();
        store
            .delivery_ack(session, &r(delivery), true, None)
            .unwrap();
    }
    fn recipient(key: &str, session: &str, delivery: &str) -> NewConversationRecipient {
        NewConversationRecipient {
            participant_key: key.into(),
            agent_session: r(session),
            delivery_ref: r(delivery),
            agent_ref: Some(format!("agent/{key}")),
            route: None,
        }
    }

    #[test]
    fn each_recipient_reads_only_its_own_reply_even_when_both_run_at_once() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let (ada, ash) = (r("agent-session/ada"), r("agent-session/ash"));
        let request = r("conversation/q1");
        store
            .create_conversation(&request, "d1", &json!({"flow":{"location":{"ref":"central:path:/x:Control/user/flows/f.html"},"entry_id":"e-q"}}),
                &[recipient("p-ada", ada.as_str(), "delivery/ada-1"), recipient("p-ash", ash.as_str(), "delivery/ash-1")])
            .unwrap();
        dispatch(&store, &ada, "delivery/ada-1", "g-ada");
        dispatch(&store, &ash, "delivery/ash-1", "g-ash");
        // Interleaved streaming, plus an unrelated assistant event on a session
        // with no delivery in flight (attributed to nothing).
        store
            .append(&ada, &chunk("g-ada", "Ada: the claim "))
            .unwrap();
        store
            .append(&ash, &chunk("g-ash", "Ash: a counter"))
            .unwrap();
        store
            .append(&ada, &chunk("g-ada", "rests on step two."))
            .unwrap();
        store
            .append(&ash, &chunk("g-ash", "example works."))
            .unwrap();
        let mid = store.conversation(&request).unwrap().unwrap();
        assert_eq!(
            mid.recipients[0].reply.as_ref().unwrap().text,
            "Ada: the claim rests on step two."
        );
        assert!(
            !mid.recipients[0].reply.as_ref().unwrap().complete,
            "partial output is not a completion"
        );
        store.append(&ash, &turn_end("g-ash")).unwrap();
        let after = store.conversation(&request).unwrap().unwrap();
        let (a, b) = (&after.recipients[0], &after.recipients[1]);
        assert_eq!(
            b.reply.as_ref().unwrap().text,
            "Ash: a counterexample works."
        );
        assert!(b.reply.as_ref().unwrap().complete);
        assert_eq!(b.delivery.as_ref().unwrap().phase, "returned");
        assert_eq!(
            a.delivery.as_ref().unwrap().phase,
            "submitted",
            "a sibling's completion does not complete this one"
        );
        assert!(
            !a.reply.as_ref().unwrap().text.contains("Ash"),
            "no cross-contamination"
        );
    }

    #[test]
    fn an_event_outside_any_delivery_is_never_read_as_a_reply() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let ada = r("agent-session/ada");
        store
            .append(&ada, &chunk("g-old", "stray from an earlier generation"))
            .unwrap();
        dispatch(&store, &ada, "delivery/ada-1", "g-new");
        store
            .append(&ada, &chunk("g-old", "late stray after the send"))
            .unwrap();
        store
            .append(&ada, &chunk("g-new", "the real answer"))
            .unwrap();
        store.append(&ada, &turn_end("g-new")).unwrap();
        let reply = store
            .delivery_reply(&ada, &r("delivery/ada-1"))
            .unwrap()
            .unwrap();
        assert_eq!(reply.text, "the real answer");
        assert!(reply.complete);
    }

    #[test]
    fn a_request_is_idempotent_and_a_changed_one_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let body = json!({"flow":{"location":{"ref":"x"},"entry_id":"e1"}});
        let recipients = [recipient("p-ada", "agent-session/ada", "delivery/ada-1")];
        let (fresh, _) = store
            .create_conversation(&r("conversation/q"), "digest-a", &body, &recipients)
            .unwrap();
        assert!(fresh);
        let (again, reading) = store
            .create_conversation(&r("conversation/q"), "digest-a", &body, &recipients)
            .unwrap();
        assert!(!again);
        assert_eq!(reading.recipients.len(), 1);
        let refused = store
            .create_conversation(&r("conversation/q"), "digest-b", &body, &recipients)
            .unwrap_err();
        assert_eq!(refused.code(), "conversation.request_conflict");
        let twice = store
            .create_conversation(
                &r("conversation/q2"),
                "d",
                &body,
                &[
                    recipient("p-ada", "agent-session/ada", "delivery/a"),
                    recipient("p-ada", "agent-session/ash", "delivery/b"),
                ],
            )
            .unwrap_err();
        assert_eq!(twice.code(), "conversation.duplicate_recipient");
    }

    #[test]
    fn work_is_derived_from_durable_state_and_survives_a_reopen() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let ada = r("agent-session/ada");
        let request = r("conversation/q");
        {
            let store = EncounterStore::open(&home).unwrap();
            store
                .create_conversation(
                    &request,
                    "d",
                    &json!({"flow":{}}),
                    &[recipient("p-ada", ada.as_str(), "delivery/ada-1")],
                )
                .unwrap();
            assert_eq!(
                store.conversation_work(10).unwrap(),
                vec![ConversationWork::Dispatch {
                    request: request.clone(),
                    participant: "p-ada".into()
                }]
            );
            dispatch(&store, &ada, "delivery/ada-1", "g1");
            store
                .conversation_set_dispatch(&request, "p-ada", "sent", None)
                .unwrap();
            assert!(
                store.conversation_work(10).unwrap().is_empty(),
                "nothing to do until the turn returns"
            );
            store.append(&ada, &chunk("g1", "answer")).unwrap();
            store.append(&ada, &turn_end("g1")).unwrap();
        }
        // The owner died; a fresh one sees the same remaining work.
        let store = EncounterStore::open(&home).unwrap();
        assert_eq!(
            store.conversation_work(10).unwrap(),
            vec![ConversationWork::Incorporate {
                request: request.clone(),
                participant: "p-ada".into()
            }]
        );
        store
            .conversation_record_inclusion(
                &request,
                "p-ada",
                "failed",
                None,
                None,
                Some("owner unavailable"),
            )
            .unwrap();
        assert_eq!(
            store.conversation_work(10).unwrap().len(),
            1,
            "a failed inclusion is retried, not forgotten"
        );
        store
            .conversation_record_inclusion(
                &request,
                "p-ada",
                "included",
                Some("e-42"),
                Some("r7"),
                None,
            )
            .unwrap();
        assert!(store.conversation_work(10).unwrap().is_empty());
        // `included` is terminal: a late failure report cannot undo it.
        store
            .conversation_record_inclusion(&request, "p-ada", "failed", None, None, Some("stale"))
            .unwrap();
        let done = store.conversation(&request).unwrap().unwrap();
        assert_eq!(done.recipients[0].inclusion, "included");
        assert_eq!(done.recipients[0].entry_id.as_deref(), Some("e-42"));
    }

    #[test]
    fn a_turn_end_wakes_the_registered_worker_once_per_turn() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        store.on_turn_ended(move || {
            seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let ada = r("agent-session/ada");
        dispatch(&store, &ada, "delivery/ada-1", "g1");
        store.append(&ada, &chunk("g1", "partial")).unwrap();
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "partial output does not wake incorporation"
        );
        store.append(&ada, &turn_end("g1")).unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_long_reply_discloses_its_bound_instead_of_truncating_silently() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let ada = r("agent-session/ada");
        dispatch(&store, &ada, "delivery/ada-1", "g1");
        let piece = "é".repeat(64 * 1024); // 128 KiB per chunk, multi-byte
        for _ in 0..6 {
            store.append(&ada, &chunk("g1", &piece)).unwrap();
        }
        store.append(&ada, &turn_end("g1")).unwrap();
        let reply = store
            .delivery_reply(&ada, &r("delivery/ada-1"))
            .unwrap()
            .unwrap();
        assert!(reply.truncated && reply.complete);
        assert_eq!(reply.bytes, 6 * 128 * 1024);
        assert!(
            reply.text.len() <= REPLY_LIMIT_BYTES && reply.text.chars().all(|c| c == 'é'),
            "cut on a character boundary"
        );
    }
    #[test]
    fn a_conversation_delivery_with_no_live_continuation_is_settled_from_the_journal_and_the_slot_is_released(
    ) {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let (dead, live, ended, plain) = (
            r("agent-session/dead"),
            r("agent-session/live"),
            r("agent-session/ended"),
            r("agent-session/plain"),
        );
        let request = r("conversation/q");
        store
            .create_conversation(
                &request,
                "d",
                &json!({"flow":{}}),
                &[
                    recipient("p-dead", dead.as_str(), "delivery/dead-1"),
                    recipient("p-live", live.as_str(), "delivery/live-1"),
                    recipient("p-ended", ended.as_str(), "delivery/ended-1"),
                ],
            )
            .unwrap();
        for (session, delivery, generation) in [
            (&dead, "delivery/dead-1", "g-dead"),
            (&live, "delivery/live-1", "g-live"),
            (&ended, "delivery/ended-1", "g-ended"),
            (&plain, "delivery/plain-1", "g-plain"),
        ] {
            dispatch(&store, session, delivery, generation);
            store
                .append(session, &chunk(generation, "partial "))
                .unwrap();
        }
        // A turn that ended is finished by the journal itself.
        store.append(&ended, &turn_end("g-ended")).unwrap();
        let mut carried = std::collections::BTreeSet::new();
        carried.insert("g-live".to_owned());
        let settled = store.conversation_reconcile_lost(&carried).unwrap();
        assert_eq!(
            settled.len(),
            1,
            "only the lost, unfinished, conversation delivery: {settled:?}"
        );
        let lost = store
            .delivery(&dead, &r("delivery/dead-1"))
            .unwrap()
            .unwrap();
        assert_eq!(lost.phase, "reconciled-no-replay");
        let detail = lost.detail.unwrap();
        for named in [
            "g-dead",
            "agent-session/dead",
            "not replayed",
            "no terminal event",
            "native session n,",
        ] {
            assert!(detail.contains(named), "{named}: {detail}");
        }
        assert_eq!(
            store
                .delivery(&live, &r("delivery/live-1"))
                .unwrap()
                .unwrap()
                .phase,
            "submitted",
            "a continuation this owner carries is untouched"
        );
        assert_eq!(
            store
                .delivery(&ended, &r("delivery/ended-1"))
                .unwrap()
                .unwrap()
                .phase,
            "returned"
        );
        assert_eq!(
            store
                .delivery(&plain, &r("delivery/plain-1"))
                .unwrap()
                .unwrap()
                .phase,
            "submitted",
            "a delivery that is not a conversation recipient's is not this reconciliation's"
        );
        // The session's single slot is free again; the lost one is not resent.
        store
            .reserve_delivery(
                &dead,
                &r("delivery/dead-2"),
                &r("human:ann"),
                &json!({"connection_generation": "g-new"}),
            )
            .unwrap();
        assert_eq!(
            store
                .delivery(&dead, &r("delivery/dead-1"))
                .unwrap()
                .unwrap()
                .phase,
            "reconciled-no-replay"
        );
        // Settling twice changes nothing.
        assert!(store
            .conversation_reconcile_lost(&carried)
            .unwrap()
            .is_empty());
        let reading = store.conversation(&request).unwrap().unwrap();
        assert!(reading.recipients[0]
            .reply
            .as_ref()
            .is_some_and(|reply| !reply.complete));
    }

    #[test]
    fn a_queued_conversation_delivery_is_found_by_its_session_and_can_be_refused_before_it_runs() {
        let root = tempfile::tempdir().unwrap();
        let store = EncounterStore::open(&AikitHome::at(root.path())).unwrap();
        let ada = r("agent-session/ada");
        let request = r("conversation/q");
        store
            .create_conversation(
                &request,
                "d",
                &json!({"flow":{}}),
                &[recipient("p-ada", ada.as_str(), "delivery/ada-1")],
            )
            .unwrap();
        store
            .queue_delivery(
                &ada,
                &r("delivery/ada-1"),
                &r("human:ann"),
                &json!({"queued": true}),
            )
            .unwrap();
        assert!(
            store.conversation_queued_sessions().unwrap().is_empty(),
            "not until it is dispatched-and-waiting"
        );
        store
            .conversation_set_dispatch(&request, "p-ada", "sent", Some("queued"))
            .unwrap();
        assert_eq!(
            store.conversation_queued_sessions().unwrap(),
            vec![ada.clone()]
        );
        assert_eq!(
            store
                .conversation_recipient_for_delivery(&ada, &r("delivery/ada-1"))
                .unwrap(),
            Some((request.clone(), "p-ada".to_owned()))
        );
        store
            .conversation_refuse_queued(
                &request,
                "p-ada",
                "conversation.participant_left: Ada has left",
            )
            .unwrap();
        let reading = store.conversation(&request).unwrap().unwrap();
        assert_eq!(reading.recipients[0].dispatch, "refused");
        assert_eq!(reading.recipients[0].inclusion, "refused");
        assert!(store.conversation_work(10).unwrap().is_empty());
    }

    #[test]
    fn document_pin_replay_is_atomic_and_survives_actual_store_reopen() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let request = r("conversation/document-pin-restart");
        let recipients = [recipient(
            "p-ada",
            "agent-session/ada",
            "delivery/document-pin",
        )];
        let body = json!({"flow":{"location":{"ref":"x"},"document_id":"5c347cc8-4926-42cf-919c-1e892681c6a8"}});
        {
            let store = EncounterStore::open(&home).unwrap();
            assert!(
                store
                    .create_conversation(&request, "unchanged-legacy-digest", &body, &recipients)
                    .unwrap()
                    .0
            );
        }
        let store = EncounterStore::open(&home).unwrap();
        let retained = store.conversation(&request).unwrap().unwrap();
        assert_eq!(retained.body, body);
        assert!(
            !store
                .create_conversation(&request, "unchanged-legacy-digest", &body, &recipients)
                .unwrap()
                .0
        );
        for document_id in [Value::Null, json!("b3243d85-2e4b-43fa-9a23-69ad507d3487")] {
            let mut changed = body.clone();
            changed["flow"]["document_id"] = document_id;
            let refused = store
                .create_conversation(&request, "unchanged-legacy-digest", &changed, &recipients)
                .unwrap_err();
            assert_eq!(refused.code(), "conversation.request_conflict");
            assert_eq!(store.conversation(&request).unwrap().unwrap().body, body);
        }
        let legacy = json!({"flow":{"location":{"ref":"legacy"}}});
        let legacy_ref = r("conversation/legacy-document-pin");
        assert!(
            store
                .create_conversation(&legacy_ref, "legacy-digest", &legacy, &recipients)
                .unwrap()
                .0
        );
        let mut added = legacy.clone();
        added["flow"]["document_id"] = body["flow"]["document_id"].clone();
        assert_eq!(
            store
                .create_conversation(&legacy_ref, "legacy-digest", &added, &recipients)
                .unwrap_err()
                .code(),
            "conversation.request_conflict"
        );
        let mut null = legacy.clone();
        null["flow"]["document_id"] = Value::Null;
        assert!(
            !store
                .create_conversation(&legacy_ref, "legacy-digest", &null, &recipients)
                .unwrap()
                .0
        );
        assert_eq!(
            store.conversation(&legacy_ref).unwrap().unwrap().body,
            legacy
        );
    }

    #[test]
    fn concurrent_store_connections_cannot_swap_optional_pin_under_one_identity() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        drop(EncounterStore::open(&home).unwrap());
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let mut starters = Vec::new();
        let mut handles = Vec::new();
        for pin in [
            "5c347cc8-4926-42cf-919c-1e892681c6a8",
            "b3243d85-2e4b-43fa-9a23-69ad507d3487",
        ] {
            let path = root.path().to_path_buf();
            let ready = ready_tx.clone();
            let (start_tx, start_rx) = std::sync::mpsc::channel();
            starters.push(start_tx);
            handles.push(std::thread::spawn(move || {
                let store = EncounterStore::open(&AikitHome::at(&path)).unwrap();
                ready.send(()).unwrap();
                start_rx
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
                let body = json!({"flow":{"location":{"ref":"x"},"document_id":pin}});
                let recipients = [recipient(
                    "p-ada",
                    "agent-session/ada",
                    "delivery/document-pin-concurrent",
                )];
                (
                    pin,
                    store.create_conversation(
                        &r("conversation/document-pin-concurrent"),
                        "same-legacy-digest",
                        &body,
                        &recipients,
                    ),
                )
            }));
        }
        for _ in 0..2 {
            ready_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
        }
        for start in starters {
            start.send(()).unwrap();
        }
        let mut winner = None;
        let mut refused = 0;
        for handle in handles {
            let (pin, result) = handle.join().unwrap();
            match result {
                Ok((fresh, _)) => {
                    assert!(fresh);
                    assert!(winner.replace(pin).is_none());
                }
                Err(error) => {
                    assert_eq!(error.code(), "conversation.request_conflict");
                    refused += 1;
                }
            }
        }
        assert_eq!(refused, 1);
        let store = EncounterStore::open(&home).unwrap();
        let reading = store
            .conversation(&r("conversation/document-pin-concurrent"))
            .unwrap()
            .unwrap();
        assert_eq!(reading.body["flow"]["document_id"].as_str(), winner);
        assert_eq!(reading.recipients.len(), 1);
    }
}
