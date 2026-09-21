//! Durable encounter history and one composer per canonical AgentSession.
//! Provider events are appended by AIKit's resident host, never by a UI client.
//! Reads use bounded cursor pages; turn duration does not determine memory use.
use crate::{AikitHome, ContextLock, LockOptions};
use aikit_core::{AikitError, ResourceRef, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterDraft {
    pub revision: u64,
    pub text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterEvent {
    pub cursor: u64,
    pub event: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncounterPage {
    pub agent_session: ResourceRef,
    pub events: Vec<EncounterEvent>,
    pub next_cursor: u64,
    pub more: bool,
    pub draft: EncounterDraft,
}

#[path = "encounter_delivery.rs"]
mod delivery;
pub use delivery::{DeliveryReservation, EncounterDelivery};
#[path = "encounter_context.rs"]
pub mod context;

pub struct EncounterStore {
    connection: Mutex<Connection>,
}
fn failure(error: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.storage", error.to_string())
}
impl EncounterStore {
    pub fn open(home: &AikitHome) -> Result<Self> {
        let _lock = ContextLock::acquire(
            home,
            "encounters-open",
            LockOptions::default().with_purpose("open canonical encounter history"),
        )?;
        std::fs::create_dir_all(home.state()).map_err(failure)?;
        let connection =
            Connection::open(home.state().join("encounters.sqlite3")).map_err(failure)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(failure)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS encounter_drafts(session TEXT PRIMARY KEY,revision INTEGER NOT NULL,text TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS encounter_events(cursor INTEGER PRIMARY KEY AUTOINCREMENT,session TEXT NOT NULL,event TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS encounter_event_session_cursor ON encounter_events(session,cursor);
            CREATE TABLE IF NOT EXISTS encounter_blocks(id INTEGER PRIMARY KEY AUTOINCREMENT,session TEXT NOT NULL,kind TEXT NOT NULL,text TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS encounter_block_session ON encounter_blocks(session,id);
            CREATE TABLE IF NOT EXISTS encounter_block_exclusions(session TEXT NOT NULL,block_id INTEGER NOT NULL,basis TEXT NOT NULL,PRIMARY KEY(session,block_id));").map_err(failure)?;
        delivery::install(&connection)?;
        context::install(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }
    pub fn append(&self, session: &ResourceRef, event: &Value) -> Result<u64> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let event = delivery::attribute(&transaction, session, event)?;
        let body = serde_json::to_string(&event).map_err(failure)?;
        transaction
            .execute(
                "INSERT INTO encounter_events(session,event) VALUES(?1,?2)",
                params![session.as_str(), body],
            )
            .map_err(failure)?;
        let cursor = transaction.last_insert_rowid() as u64;
        project_block(&transaction, session, &event)?;
        delivery::finish(&transaction, session, &event, cursor)?;
        transaction.commit().map_err(failure)?;
        Ok(cursor)
    }
    /// Bounded encounter presentation from the canonical owner journal.
    pub fn view(&self, session: &ResourceRef, before: Option<u64>) -> Result<Value> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let mut query=connection.prepare("SELECT id,CASE WHEN kind='assistant' AND NOT EXISTS(SELECT 1 FROM encounter_blocks AS earlier WHERE earlier.session=encounter_blocks.session AND earlier.kind='user' AND earlier.id<encounter_blocks.id) THEN 'provider-notice' ELSE kind END,text FROM encounter_blocks WHERE session=?1 AND id<?2 AND NOT EXISTS(SELECT 1 FROM encounter_block_exclusions AS excluded WHERE excluded.session=encounter_blocks.session AND excluded.block_id=encounter_blocks.id) ORDER BY id DESC LIMIT 17").map_err(failure)?;
        let rows=query.query_map(params![session.as_str(),before.unwrap_or(i64::MAX as u64)],|row|Ok(serde_json::json!({"id":row.get::<_,u64>(0)?,"kind":row.get::<_,String>(1)?,"text":row.get::<_,String>(2)?}))).map_err(failure)?;
        let mut blocks = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(failure)?;
        let more = blocks.len() > 16;
        blocks.truncate(16);
        blocks.reverse();
        Ok(
            serde_json::json!({"agent_session":session,"blocks":blocks,"more":more,"draft":draft_in(&connection,session)?,"prepared_context_receipts":context::receipts_in(&connection,session)?}),
        )
    }

    /// Fail-closed, presentation-only classification of a legacy native-load
    /// replay. Canonical journal events and stored blocks remain untouched.
    pub fn classify_legacy_load_replay(&self, session: &ResourceRef) -> Result<Value> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let events = journal_events(&connection, session)?;
        let blocks = journal_blocks(&connection, session)?;
        let mut output = Vec::new();
        for (at, event) in events.iter().enumerate() {
            let Some(native) = event
                .event
                .pointer("/receipt/native_session_id")
                .and_then(Value::as_str)
                .filter(|_| event.event["kind"] == "owner-shutdown-completed")
            else {
                continue;
            };
            let mut generation = None;
            let mut replay_cursor = None;
            let mut binding_cursor = None;
            let mut ambiguous = false;
            for next in &events[at + 1..] {
                if next.event["kind"] == "user-message" {
                    ambiguous = true;
                    break;
                }
                if next.event["kind"] == "binding"
                    && next.event["continuation"] == "native-load"
                    && next.event["native_session_id"].as_str() == Some(native)
                {
                    binding_cursor = Some(next.cursor);
                    break;
                }
                let Some(signal) = next.event.pointer("/event/Signal") else {
                    if next.event["kind"].as_str().is_some() {
                        ambiguous = true
                    };
                    continue;
                };
                if signal["native_session_id"].as_str() != Some(native) {
                    ambiguous = true;
                    break;
                }
                let Some(current) = next.event["connection_generation"].as_str() else {
                    ambiguous = true;
                    break;
                };
                if generation.as_deref().is_some_and(|value| value != current) {
                    ambiguous = true;
                    break;
                };
                generation.get_or_insert_with(|| current.to_owned());
                match signal.pointer("/kind/kind").and_then(Value::as_str) {
                    Some("status") => {}
                    Some("agent-message-chunk") => {
                        if replay_cursor.replace(next.cursor).is_some() {
                            ambiguous = true;
                            break;
                        }
                    }
                    _ => {
                        ambiguous = true;
                        break;
                    }
                }
            }
            let (Some(replay_cursor), Some(binding_cursor), Some(generation)) =
                (replay_cursor, binding_cursor, generation)
            else {
                continue;
            };
            if ambiguous {
                continue;
            }
            let Some(block_index) = projected_assistant_block_index(&events, replay_cursor) else {
                continue;
            };
            let projected = projected_blocks(&events);
            if projected.len() != blocks.len()
                || !projected.iter().zip(&blocks).all(
                    |((kind, text), (_, actual_kind, actual_text))| {
                        kind == actual_kind && text == actual_text
                    },
                )
            {
                continue;
            }
            let Some((block_id, kind, text)) = blocks.get(block_index) else {
                continue;
            };
            let expected = events
                .iter()
                .find(|item| item.cursor == replay_cursor)
                .and_then(|item| item.event.pointer("/event/Signal/kind/text"))
                .and_then(Value::as_str);
            if kind != "assistant" || expected != Some(text.as_str()) {
                continue;
            }
            let basis = serde_json::json!({"kind":"legacy-native-load-replay","standing":"derived-from-exact-owner-journal-causality","shutdown_completed_cursor":event.cursor,"native_session_id":native,"connection_generation":generation,"pre_native_load_provider_cursors":[replay_cursor],"native_load_binding_cursor":binding_cursor,"projection_block_id":block_id,"intervening_owner_prompt_or_user_write":false});
            output.push(basis);
        }
        // Replace, rather than accumulate, this derived presentation overlay.
        // The raw journal and persisted blocks are never mutated.
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        transaction
            .execute(
                "DELETE FROM encounter_block_exclusions WHERE session=?1",
                params![session.as_str()],
            )
            .map_err(failure)?;
        for basis in &output {
            transaction.execute("INSERT INTO encounter_block_exclusions(session,block_id,basis) VALUES(?1,?2,?3)", params![session.as_str(), basis["projection_block_id"].as_u64(), serde_json::to_string(basis).map_err(failure)?]).map_err(failure)?;
        }
        transaction.commit().map_err(failure)?;
        Ok(serde_json::json!({"classified":!output.is_empty(),"receipts":output}))
    }
    pub fn legacy_load_reclassifications(&self, session: &ResourceRef) -> Result<Vec<Value>> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let mut q = connection
            .prepare(
                "SELECT basis FROM encounter_block_exclusions WHERE session=?1 ORDER BY block_id",
            )
            .map_err(failure)?;
        let rows = q
            .query_map(params![session.as_str()], |r| r.get::<_, String>(0))
            .map_err(failure)?;
        rows.map(|r| serde_json::from_str(&r.map_err(failure)?).map_err(failure))
            .collect()
    }
    pub fn draft(&self, session: &ResourceRef) -> Result<EncounterDraft> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        draft_in(&connection, session)
    }
    pub fn set_draft(
        &self,
        session: &ResourceRef,
        basis: u64,
        text: &str,
    ) -> Result<EncounterDraft> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let held = draft_in(&transaction, session)?;
        if held.revision != basis {
            return Err(AikitError::new(
                "encounter.draft_conflict",
                "The canonical composer changed in another view; reread before editing.",
            ));
        }
        let revision = basis
            .checked_add(1)
            .ok_or_else(|| failure("draft revision exhausted"))?;
        transaction.execute("INSERT INTO encounter_drafts(session,revision,text) VALUES(?1,?2,?3) ON CONFLICT(session) DO UPDATE SET revision=excluded.revision,text=excluded.text",params![session.as_str(),revision,text]).map_err(failure)?;
        transaction.commit().map_err(failure)?;
        Ok(EncounterDraft {
            revision,
            text: text.into(),
        })
    }
    /// Serialise dispatch with the canonical draft and journal. Provider sink
    /// writes wait on this transaction, so no response can precede its prompt.
    pub fn submit(
        &self,
        session: &ResourceRef,
        basis: u64,
        dispatch: impl FnOnce(&str) -> Result<()>,
    ) -> Result<EncounterDraft> {
        self.submit_context(session, basis, None, dispatch)
    }
    pub fn submit_context(
        &self,
        session: &ResourceRef,
        basis: u64,
        expectation: Option<&context::ContextExpectation>,
        dispatch: impl FnOnce(&str) -> Result<()>,
    ) -> Result<EncounterDraft> {
        validate(session)?;
        let mut connection = self.connection.lock().map_err(failure)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        let draft = draft_in(&transaction, session)?;
        if draft.revision != basis || draft.text.trim().is_empty() {
            return Err(AikitError::new(
                "encounter.draft_conflict",
                "Prompt requires the current nonempty canonical draft",
            ));
        }
        let revision = basis
            .checked_add(1)
            .ok_or_else(|| failure("draft revision exhausted"))?;
        let prepared = context::prepare_submission(&transaction, session, expectation)?;
        let payload = context::compose(&draft.text, prepared.as_ref())?;
        dispatch(&payload)?;
        let accepted = (|| -> Result<()> {
            transaction.execute("INSERT INTO encounter_events(session,event) VALUES(?1,?2)",params![session.as_str(),serde_json::to_string(&serde_json::json!({"kind":"user-message","text":draft.text,"draft_revision":basis,"prepared_context":prepared,"payload_digest":format!("blake3:{}",blake3::hash(payload.as_bytes()).to_hex())})).map_err(failure)?]).map_err(failure)?;
            project_block(
                &transaction,
                session,
                &serde_json::json!({"kind":"user-message","text":draft.text}),
            )?;
            transaction
                .execute(
                    "UPDATE encounter_drafts SET revision=?2,text='' WHERE session=?1",
                    params![session.as_str(), revision],
                )
                .map_err(failure)?;
            context::clear_submitted(&transaction, prepared.as_ref())?;
            transaction.commit().map_err(failure)
        })();
        accepted.map_err(|error|AikitError::new("encounter.submission_uncertain",format!("Provider accepted the prompt but canonical persistence failed; do not resend automatically: {error}")))?;
        Ok(EncounterDraft {
            revision,
            text: String::new(),
        })
    }
    pub fn events(&self, session: &ResourceRef, after: u64, limit: usize) -> Result<EncounterPage> {
        validate(session)?;
        let connection = self.connection.lock().map_err(failure)?;
        let limit = limit.clamp(1, 256);
        let mut query=connection.prepare("SELECT cursor,event FROM encounter_events WHERE session=?1 AND cursor>?2 ORDER BY cursor LIMIT ?3").map_err(failure)?;
        let rows = query
            .query_map(params![session.as_str(), after, limit + 1], |row| {
                Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(failure)?;
        let mut events = Vec::with_capacity(limit);
        let mut more = false;
        let mut bytes = 0;
        for row in rows {
            let (cursor, body) = row.map_err(failure)?;
            if events.len() == limit || (!events.is_empty() && bytes + body.len() > 256 * 1024) {
                more = true;
                break;
            }
            bytes += body.len();
            events.push(EncounterEvent {
                cursor,
                event: serde_json::from_str(&body).map_err(failure)?,
            });
        }
        let next_cursor = events.last().map(|e| e.cursor).unwrap_or(after);
        Ok(EncounterPage {
            agent_session: session.clone(),
            events,
            next_cursor,
            more,
            draft: draft_in(&connection, session)?,
        })
    }
}
fn journal_events(connection: &Connection, session: &ResourceRef) -> Result<Vec<EncounterEvent>> {
    let mut q = connection
        .prepare("SELECT cursor,event FROM encounter_events WHERE session=?1 ORDER BY cursor")
        .map_err(failure)?;
    let rows = q
        .query_map(params![session.as_str()], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(failure)?;
    let pairs = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(failure)?;
    pairs
        .into_iter()
        .map(|(cursor, event)| {
            Ok(EncounterEvent {
                cursor,
                event: serde_json::from_str(&event).map_err(failure)?,
            })
        })
        .collect()
}

fn journal_blocks(
    connection: &Connection,
    session: &ResourceRef,
) -> Result<Vec<(u64, String, String)>> {
    let mut q = connection
        .prepare("SELECT id,kind,text FROM encounter_blocks WHERE session=?1 ORDER BY id")
        .map_err(failure)?;
    let rows = q
        .query_map(params![session.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .map_err(failure)?;
    let result = rows
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(failure)?;
    Ok(result)
}

fn projected_blocks(events: &[EncounterEvent]) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    for event in events {
        let piece = if event.event["kind"] == "user-message" {
            event.event["text"].as_str().map(|text| ("user", text))
        } else if let Some(kind) = event.event.pointer("/event/Signal/kind") {
            match kind["kind"].as_str() {
                Some("agent-message-chunk") => {
                    kind["text"].as_str().map(|text| ("assistant", text))
                }
                Some("agent-thought-chunk") => kind["text"].as_str().map(|text| ("thinking", text)),
                _ => None,
            }
        } else if event
            .event
            .pointer("/event/TurnEnded/stop/Completed")
            .is_some()
        {
            Some(("completed", ""))
        } else {
            None
        };
        let Some((kind, text)) = piece else { continue };
        if matches!(kind, "assistant" | "thinking")
            && blocks
                .last()
                .is_some_and(|(held, prior): &(String, String)| {
                    held == kind && prior.len() + text.len() <= 16 * 1024
                })
        {
            blocks.last_mut().unwrap().1.push_str(text);
        } else {
            blocks.push((kind.into(), text.into()));
        }
    }
    blocks
}

fn projected_assistant_block_index(events: &[EncounterEvent], target: u64) -> Option<usize> {
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut target_index = None;
    for event in events {
        let piece = if event.event["kind"] == "user-message" {
            Some(("user", event.event["text"].as_str()?))
        } else if let Some(kind) = event.event.pointer("/event/Signal/kind") {
            match kind["kind"].as_str() {
                Some("agent-message-chunk") => Some(("assistant", kind["text"].as_str()?)),
                Some("agent-thought-chunk") => Some(("thinking", kind["text"].as_str()?)),
                _ => None,
            }
        } else if event
            .event
            .pointer("/event/TurnEnded/stop/Completed")
            .is_some()
        {
            Some(("completed", ""))
        } else {
            None
        };
        let Some((kind, text)) = piece else { continue };
        let index = if matches!(kind, "assistant" | "thinking")
            && blocks
                .last()
                .is_some_and(|(held, prior)| held == kind && prior.len() + text.len() <= 16 * 1024)
        {
            let i = blocks.len() - 1;
            blocks[i].1.push_str(text);
            i
        } else {
            blocks.push((kind.into(), text.into()));
            blocks.len() - 1
        };
        if event.cursor == target {
            target_index = Some(index)
        }
    }
    target_index
}

fn validate(session: &ResourceRef) -> Result<()> {
    if !session.as_str().starts_with("agent-session/") {
        return Err(AikitError::new(
            "encounter.identity",
            "Encounter history requires a canonical AgentSession ref",
        ));
    }
    Ok(())
}
fn draft_in(connection: &Connection, session: &ResourceRef) -> Result<EncounterDraft> {
    Ok(connection
        .query_row(
            "SELECT revision,text FROM encounter_drafts WHERE session=?1",
            params![session.as_str()],
            |row| {
                Ok(EncounterDraft {
                    revision: row.get(0)?,
                    text: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(failure)?
        .unwrap_or(EncounterDraft {
            revision: 0,
            text: String::new(),
        }))
}

fn project_block(connection: &Connection, session: &ResourceRef, event: &Value) -> Result<()> {
    let (kind, text) = if event["kind"] == "user-message" {
        ("user", event["text"].as_str().unwrap_or("").to_owned())
    } else if let Some(signal) = event.pointer("/event/Signal/kind") {
        match signal["kind"].as_str() {
            Some("agent-message-chunk") => (
                "assistant",
                signal["text"].as_str().unwrap_or("").to_owned(),
            ),
            Some("agent-thought-chunk") => {
                ("thinking", signal["text"].as_str().unwrap_or("").to_owned())
            }
            Some("tool-call") => (
                "tool",
                serde_json::to_string(&signal["payload"]).map_err(failure)?,
            ),
            Some("permission-requested") => (
                "permission",
                serde_json::to_string(&signal["request"]).map_err(failure)?,
            ),
            _ => return Ok(()),
        }
    } else if let Some(stop) = event.pointer("/event/TurnEnded/stop") {
        if stop.get("Completed").is_some() {
            ("completed", String::new())
        } else if stop.get("Cancelled").is_some() {
            ("cancelled", "Stopped by request".into())
        } else {
            ("error", serde_json::to_string(stop).map_err(failure)?)
        }
    } else {
        return Ok(());
    };
    // Chunks are bounded by bytes, without splitting UTF-8 code points. Adjacent
    // exposed thinking/message updates coalesce only within this storage block.
    let mut remaining = text.as_str();
    loop {
        let mut end = remaining.len().min(16 * 1024);
        while !remaining.is_char_boundary(end) {
            end -= 1;
        }
        let part = &remaining[..end];
        let last:Option<(u64,String,String)>=connection.query_row("SELECT id,kind,text FROM encounter_blocks WHERE session=?1 ORDER BY id DESC LIMIT 1",params![session.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(failure)?;
        if let Some((id, held_kind, held_text)) = last.filter(|(_, held_kind, held_text)| {
            matches!(kind, "assistant" | "thinking")
                && held_kind == kind
                && held_text.len() + part.len() <= 16 * 1024
        }) {
            let _ = held_kind;
            connection
                .execute(
                    "UPDATE encounter_blocks SET text=?2 WHERE id=?1",
                    params![id, held_text + part],
                )
                .map_err(failure)?;
        } else {
            connection
                .execute(
                    "INSERT INTO encounter_blocks(session,kind,text) VALUES(?1,?2,?3)",
                    params![session.as_str(), kind, part],
                )
                .map_err(failure)?;
        }
        remaining = &remaining[end..];
        if remaining.is_empty() {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_sqlite_resource_failure_preserves_draft_and_refuses_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/resource-failure").unwrap();
        let store = EncounterStore::open(&home).unwrap();
        store.set_draft(&session, 0, "keep this composer").unwrap();
        // SQLite's actual read-only mode rejects writes at the storage engine.
        store
            .connection
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only=ON")
            .unwrap();
        let append = store
            .append(
                &session,
                &serde_json::json!({"kind":"user-message","text":"cannot persist"}),
            )
            .unwrap_err();
        assert_eq!(append.code(), "encounter.storage");
        let mut dispatched = false;
        let result = store
            .submit(&session, 1, |_| {
                dispatched = true;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(result.code(), "encounter.storage");
        assert!(!dispatched);
        assert_eq!(store.draft(&session).unwrap().text, "keep this composer");
        assert!(store.events(&session, 0, 10).unwrap().events.is_empty());
    }
    #[test]
    fn concurrent_real_connections_enforce_single_composer_basis() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let first = EncounterStore::open(&home).unwrap();
        let second = EncounterStore::open(&home).unwrap();
        let session = ResourceRef::parse("agent-session/concurrent-draft").unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, store)| {
                let session = session.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.set_draft(&session, 0, &format!("writer {index}"))
                })
            })
            .collect::<Vec<_>>();
        let results = workers
            .into_iter()
            .map(|w| w.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|r| r
                    .as_ref()
                    .is_err_and(|e| e.code() == "encounter.draft_conflict"))
                .count(),
            1
        );
    }
    #[test]
    fn actual_history_pages_survive_reopen_and_composer_conflicts() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/history-real").unwrap();
        let other = ResourceRef::parse("agent-session/other").unwrap();
        let store = EncounterStore::open(&home).unwrap();
        for sequence in 0..2048 {
            store
                .append(
                    &session,
                    &serde_json::json!({"thinking":"thinking ∆","sequence":sequence}),
                )
                .unwrap();
        }
        store
            .append(&other, &serde_json::json!({"text":"other encounter"}))
            .unwrap();
        let draft = store.set_draft(&session, 0, "one shared draft").unwrap();
        assert_eq!(draft.revision, 1);
        assert!(store.set_draft(&session, 0, "stale").is_err());
        drop(store);
        let reopened = EncounterStore::open(&home).unwrap();
        let mut cursor = 0;
        let mut count = 0;
        loop {
            let page = reopened.events(&session, cursor, 73).unwrap();
            assert!(page.events.len() <= 73);
            assert_eq!(page.draft.text, "one shared draft");
            for e in &page.events {
                assert_eq!(e.event["sequence"], count);
                assert_eq!(e.event["thinking"], "thinking ∆");
                count += 1;
            }
            cursor = page.next_cursor;
            if !page.more {
                break;
            }
        }
        assert_eq!(count, 2048);
        assert!(reopened.draft(&other).unwrap().text.is_empty());
    }
    fn legacy_event(native: &str, generation: &str, kind: &str, text: Option<&str>) -> Value {
        let mut signal =
            serde_json::json!({"native_session_id":native,"sequence":1,"kind":{"kind":kind}});
        if let Some(text) = text {
            signal["kind"]["text"] = serde_json::json!(text);
        }
        serde_json::json!({"kind":"provider","connection_generation":generation,"event":{"Signal":signal}})
    }
    fn install_legacy_window(
        store: &EncounterStore,
        session: &ResourceRef,
        provider_native: &str,
        replay_native: &str,
        with_user: bool,
    ) {
        store
            .append(
                session,
                &serde_json::json!({"kind":"user-message","text":"original"}),
            )
            .unwrap();
        store
            .append(
                session,
                &legacy_event(
                    provider_native,
                    "old",
                    "agent-message-chunk",
                    Some("answer"),
                ),
            )
            .unwrap();
        store.append(session,&serde_json::json!({"kind":"provider","event":{"TurnEnded":{"stop":{"Completed":{}}}}})).unwrap();
        store.append(session,&serde_json::json!({"kind":"owner-shutdown-completed","receipt":{"native_session_id":provider_native}})).unwrap();
        store
            .append(
                session,
                &legacy_event(replay_native, "load", "status", None),
            )
            .unwrap();
        if with_user {
            store
                .append(
                    session,
                    &serde_json::json!({"kind":"user-message","text":"intervening"}),
                )
                .unwrap();
        }
        store
            .append(
                session,
                &legacy_event(replay_native, "load", "agent-message-chunk", Some("answer")),
            )
            .unwrap();
        store.append(session,&serde_json::json!({"kind":"binding","continuation":"native-load","native_session_id":provider_native})).unwrap();
    }
    #[test]
    fn actual_sqlite_legacy_load_reclassification_hides_only_proven_replay_block() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/legacy-load").unwrap();
        let store = EncounterStore::open(&home).unwrap();
        install_legacy_window(&store, &session, "native/a", "native/a", false);
        let classified = store.classify_legacy_load_replay(&session).unwrap();
        assert_eq!(classified["classified"], true);
        let view = store.view(&session, None).unwrap();
        assert_eq!(view["blocks"].as_array().unwrap().len(), 3);
        let raw = store.events(&session, 0, 32).unwrap();
        assert_eq!(
            raw.events.len(),
            7,
            "classification preserves journal events"
        );
    }
    #[test]
    fn actual_sqlite_legacy_load_reclassification_refuses_mismatched_native_or_intervening_user() {
        for (name, replay_native, with_user) in [
            ("native-mismatch", "native/b", false),
            ("intervening-user", "native/a", true),
        ] {
            let root = tempfile::tempdir().unwrap();
            let home = AikitHome::at(root.path());
            let session = ResourceRef::parse(&format!("agent-session/{name}")).unwrap();
            let store = EncounterStore::open(&home).unwrap();
            install_legacy_window(&store, &session, "native/a", replay_native, with_user);
            assert_eq!(
                store.classify_legacy_load_replay(&session).unwrap()["classified"],
                false
            );
            assert_eq!(
                store.view(&session, None).unwrap()["blocks"]
                    .as_array()
                    .unwrap()
                    .len(),
                if with_user { 5 } else { 4 }
            );
        }
    }
    #[test]
    fn actual_sqlite_legacy_load_reclassification_refuses_unmatched_projection() {
        let root = tempfile::tempdir().unwrap();
        let home = AikitHome::at(root.path());
        let session = ResourceRef::parse("agent-session/legacy-extra-block").unwrap();
        let store = EncounterStore::open(&home).unwrap();
        install_legacy_window(&store, &session, "native/a", "native/a", false);
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO encounter_blocks(session,kind,text) VALUES(?1,?2,?3)",
                params![session.as_str(), "assistant", "answer"],
            )
            .unwrap();
        assert_eq!(
            store.classify_legacy_load_replay(&session).unwrap()["classified"],
            false
        );
        assert_eq!(
            store.view(&session, None).unwrap()["blocks"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
    }
}
