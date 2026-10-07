//! "Actual use" reading for one encounter turn: what the body was *given* and
//! what it *did*, from the receipts and journals that already exist.
//!
//! Process presence is not use. For a given turn this answers, citing the
//! source of every statement and saying plainly what could not be read:
//!
//! 1. the AIKit prepared-context version actually delivered to the turn (the
//!    `now-context-delivered` receipt in the encounter journal: version, digest,
//!    basis digest, change cursor), and any degradation or uncertainty;
//! 2. the decision provider and invocation behind that prepared view (carried
//!    by the receipt from the view's own basis), and which sources the decision
//!    selected into it (the view, read back from Redis when asked);
//! 3. the QL operations the body made: the `ql_*` tool calls and results the
//!    journal recorded, the Python `ql_relational.*` calls inside `ipython`
//!    tool calls, and the owner's own content-addressed faculty receipts
//!    (`actuation.prime-ql-operation/v1`, keyed by the agent session) that say
//!    the QL owner executed them.
//!
//! No store is added and nothing is written: this reads the encounter journal
//! (`encounters.sqlite3`), an Actuation faculty evidence directory, and, if
//! given, the Redis prepared view. A call with no receipt, or a receipt with no
//! call, is reported as exactly that.

use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{encounter::EncounterStore, AikitHome, RedisNowConfig, RedisNowStore};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const READING_SCHEMA: &str = "aikit.encounter-use-reading/v1";
const QL_TOOLS: [&str; 6] = [
    "ql_project_event",
    "ql_decision_frame",
    "ql_harmonic_read",
    "ql_decide",
    "ql_validate_determination",
    "ql_invoke",
];
const NOW_KINDS: [&str; 4] = [
    "now-context-delivered",
    "now-context-degraded",
    "now-context-delivery-uncertain",
    "now-context-cursor-uncertain",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnSelector {
    Last,
    /// 1-based, in order of the user messages.
    Index(usize),
    /// The turn whose window contains this journal cursor.
    Cursor(u64),
}

pub struct UseRequest {
    pub agent_session: ResourceRef,
    pub turn: TurnSelector,
    pub faculty_evidence: Option<PathBuf>,
    pub redis_config: Option<PathBuf>,
}

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.use_reading", message.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// The whole journal of one session, in cursor order.
fn journal(home: &AikitHome, session: &ResourceRef) -> Result<Vec<(u64, Value)>> {
    let store = EncounterStore::open(home)?;
    let mut after = 0;
    let mut out = Vec::new();
    loop {
        let page = store.events(session, after, 256)?;
        for event in page.events {
            out.push((event.cursor, event.event));
        }
        if !page.more {
            return Ok(out);
        }
        after = page.next_cursor;
    }
}

/// Faculty receipts for one trace, read exactly as the owner reads them: only
/// correctly named, content-addressed `actuation.prime-ql-operation/v1` files.
fn faculty_receipts(root: &Path, trace: &str) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(root).map_err(|e| error(format!("{}: {e}", root.display())))? {
        let path = entry.map_err(|e| error(e.to_string()))?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(receipt) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if receipt["schema"] == "actuation.prime-ql-operation/v1"
            && receipt["trace_ref"].as_str() == Some(trace)
        {
            out.push(receipt);
        }
    }
    out.sort_by_key(|r| {
        r["observed_unix_nanos"]
            .as_str()
            .and_then(|n| n.parse::<u128>().ok())
            .unwrap_or(0)
    });
    Ok(out)
}

struct Turn<'a> {
    index: usize,
    start: u64,
    /// Exclusive: the next turn's first cursor, or past the last event.
    end: u64,
    events: Vec<&'a (u64, Value)>,
}

fn signal_kind(event: &Value) -> Option<&Value> {
    event["event"]["Signal"]["kind"]
        .as_object()
        .map(|_| &event["event"]["Signal"]["kind"])
}

fn turns(events: &[(u64, Value)]) -> Vec<Turn<'_>> {
    let starts: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, (_, e))| e["kind"] == "user-message")
        .map(|(i, _)| i)
        .collect();
    starts
        .iter()
        .enumerate()
        .map(|(n, &from)| {
            let to = starts.get(n + 1).copied().unwrap_or(events.len());
            Turn {
                index: n + 1,
                start: events[from].0,
                end: events.get(to).map(|(c, _)| *c).unwrap_or(u64::MAX),
                events: events[from..to].iter().collect(),
            }
        })
        .collect()
}

fn outcome(turn: &Turn<'_>) -> &'static str {
    let mut result = "open";
    for (_, event) in &turn.events {
        if event["event"].get("TurnEnded").is_some() {
            result = "turn-ended";
        }
        if let Some(kind) = signal_kind(event).and_then(|k| k["kind"].as_str()) {
            match kind {
                "completed" => result = "completed",
                "cancelled" => result = "cancelled",
                "failed" => result = "failed",
                _ => {}
            }
        }
    }
    result
}

fn payload_text(result: &Value) -> String {
    result["result"]["content"]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Python faculty calls inside an `ipython` tool call's code.
fn python_faculty_calls(code: &str) -> Vec<String> {
    let mut calls = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("ql_relational.") {
        let tail = &rest[at + "ql_relational.".len()..];
        let name: String = tail
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            calls.push(name);
        }
        rest = tail;
    }
    calls
}

fn ql_operations(
    turn: &Turn<'_>,
    faculty: Option<&[Value]>,
    window_ms: (u64, Option<u64>),
) -> Value {
    let mut tool_calls: Vec<Value> = Vec::new();
    let mut python_calls: Vec<Value> = Vec::new();
    let mut results: std::collections::BTreeMap<String, Value> = Default::default();
    for (cursor, event) in &turn.events {
        let Some(kind) = signal_kind(event) else {
            continue;
        };
        let payload = &kind["payload"];
        let name = payload["toolName"].as_str().unwrap_or_default();
        match kind["kind"].as_str() {
            Some("tool-result") if QL_TOOLS.contains(&name) => {
                let text = payload_text(payload);
                let schema = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["schema"].as_str().map(str::to_owned));
                results.insert(
                    payload["toolCallId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    json!({
                        "cursor": cursor,
                        "is_error": payload["isError"],
                        "result_schema": schema,
                        "result_sha256": sha256_hex(text.as_bytes()),
                    }),
                );
            }
            Some("tool-call") if QL_TOOLS.contains(&name) => {
                let id = payload["toolCallId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                // A call is reported once; later updates of the same call are not repeats.
                if !tool_calls.iter().any(|c| c["tool_call_id"] == json!(id)) {
                    tool_calls.push(json!({
                        "tool": name,
                        "tool_call_id": id,
                        "cursor": cursor,
                        "operation": payload["args"]["operation"],
                        "position": payload["args"]["position"],
                    }));
                }
            }
            Some("tool-call") if name == "ipython" => {
                let id = payload["toolCallId"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let code = payload["args"]["code"]
                    .as_str()
                    .or_else(|| payload["args"]["command"].as_str())
                    .unwrap_or_default();
                for function in python_faculty_calls(code) {
                    if !python_calls
                        .iter()
                        .any(|c| c["tool_call_id"] == json!(id) && c["function"] == json!(function))
                    {
                        python_calls.push(json!({
                            "function": function, "tool_call_id": id, "cursor": cursor,
                        }));
                    }
                }
            }
            _ => {}
        }
    }
    for call in &mut tool_calls {
        let id = call["tool_call_id"].as_str().unwrap_or_default().to_owned();
        call["result"] = results
            .get(&id)
            .cloned()
            .unwrap_or(json!({"observed": false}));
    }
    let (start_ms, end_ms) = window_ms;
    let in_window: Vec<Value> = faculty
        .unwrap_or(&[])
        .iter()
        .filter(|r| {
            let ms = r["observed_unix_nanos"]
                .as_str()
                .and_then(|n| n.parse::<u128>().ok())
                .map(|n| (n / 1_000_000) as u64);
            ms.is_some_and(|ms| ms >= start_ms && end_ms.is_none_or(|end| ms < end))
        })
        .map(|r| {
            json!({
                "operation": r["operation"],
                "success": r["success"],
                "ql_mef_revision": r["ql_mef_revision"],
                "request_digest": r["request_digest"],
                "response_digest": r["response_digest"],
                "observed_unix_nanos": r["observed_unix_nanos"],
                "declared_locus_ref": r["declared_locus_ref"],
                "error": r["error"],
            })
        })
        .collect();
    let reconciliation = json!({
        "python_calls_recorded": python_calls.len(),
        "faculty_receipts_in_turn": in_window.len(),
        "statement": if python_calls.is_empty() && in_window.is_empty() {
            "no Python faculty call and no faculty receipt in this turn"
        } else if python_calls.len() == in_window.len() {
            "every recorded Python faculty call has an owner receipt in the turn window (counts agree; calls are matched by count and time, not by identity)"
        } else {
            "the number of recorded Python faculty calls and owner receipts in the turn window differ; read both lists"
        },
    });
    json!({
        "tool_calls": tool_calls,
        "python_faculty_calls": python_calls,
        "faculty_receipts": in_window,
        "reconciliation": reconciliation,
    })
}

fn prepared_context(turn: &Turn<'_>) -> Value {
    let mut delivered = Vec::new();
    let mut degraded = Vec::new();
    let mut uncertain = Vec::new();
    for (cursor, event) in &turn.events {
        let kind = event["kind"].as_str().unwrap_or_default();
        if !NOW_KINDS.contains(&kind) {
            continue;
        }
        let mut entry = event.clone();
        entry["cursor"] = json!(cursor);
        match kind {
            "now-context-delivered" => delivered.push(entry),
            "now-context-degraded" => degraded.push(entry),
            _ => uncertain.push(entry),
        }
    }
    json!({
        "state": if !delivered.is_empty() { "delivered" }
                 else if !uncertain.is_empty() { "uncertain" }
                 else if !degraded.is_empty() { "degraded-none-delivered" }
                 else { "none-selected-or-none-delivered" },
        "delivered": delivered, "degraded": degraded, "uncertain": uncertain,
        "source": "encounter journal: now-context-* events",
    })
}

/// The prepared view in Redis, compared with what the receipt says was delivered.
fn redis_reading(redis_config: &Path, delivered: &[Value]) -> Value {
    let Some(receipt) = delivered.last().map(|e| &e["receipt"]) else {
        return json!({"read": false, "reason": "no delivery receipt in this turn to compare against"});
    };
    let attempt = (|| -> Result<Value> {
        let bytes = std::fs::read(redis_config).map_err(|e| error(e.to_string()))?;
        let config: RedisNowConfig =
            serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        config.validate()?;
        let participant =
            ResourceRef::parse(receipt["participant_ref"].as_str().unwrap_or_default())?;
        let store = RedisNowStore::new(config)?;
        let expected = receipt["prepared_digest"].as_str().unwrap_or_default();
        // The reader is local, so the local view is tried first; an
        // external-provider reading is the fallback for views whose payload
        // cannot be read back locally under that provider's boundary.
        let mut last_refusal = None;
        for external in [false, true] {
            match store.read_prepared(&participant, external, None) {
                Ok(Some(view)) => {
                    if view.digest()? == expected {
                        return Ok(json!({
                            "read": true,
                            "view_is_the_delivered_one": true,
                            "version": view.version,
                            "decision_provider": view.basis.decision_provider,
                            "jev_invocation_ref": view.jev_invocation_ref,
                            "selected_source_refs": view.items.iter().map(|i| i.source_ref.clone()).collect::<Vec<_>>(),
                            "read_as": if external { "external-provider view" } else { "local view" },
                        }));
                    }
                }
                Ok(None) => {}
                Err(refusal) => last_refusal = Some(refusal.message().to_owned()),
            }
        }
        let current = store.current_version(&participant, None)?;
        Ok(json!({
            "read": true,
            "view_is_the_delivered_one": false,
            "current_version": current,
            "delivered_version": receipt["prepared_version"],
            "refusal": last_refusal,
            "note": "Redis now holds a different prepared version than the one delivered, or refused the read; the delivered view's content is not recoverable from it",
        }))
    })();
    attempt.unwrap_or_else(|e| json!({"read": false, "reason": e.message()}))
}

pub fn read(home: &AikitHome, request: &UseRequest) -> Result<Value> {
    let events = journal(home, &request.agent_session)?;
    if events.is_empty() {
        return Err(error(format!(
            "{} has no journal in this AIKit home",
            request.agent_session
        )));
    }
    assemble(&events, request)
}

/// Pure over the journal: separated so it is testable without a store.
pub(crate) fn assemble(events: &[(u64, Value)], request: &UseRequest) -> Result<Value> {
    let all = turns(events);
    if all.is_empty() {
        return Err(error(
            "the journal records no user message, so there is no turn",
        ));
    }
    let turn = match &request.turn {
        TurnSelector::Last => all.last(),
        TurnSelector::Index(n) => all.get(n.wrapping_sub(1)),
        TurnSelector::Cursor(c) => all.iter().find(|t| *c >= t.start && *c < t.end),
    }
    .ok_or_else(|| error(format!("no such turn ({} turns recorded)", all.len())))?;

    let observed = |e: &Value| e["observed_at_ms"].as_u64();
    let started_ms = turn.events.first().and_then(|(_, e)| observed(e));
    let ended_ms = turn.events.last().and_then(|(_, e)| observed(e));
    let next_start_ms = all
        .get(turn.index)
        .and_then(|next| next.events.first())
        .and_then(|(_, e)| observed(e));

    let mut absences: Vec<String> = Vec::new();
    let prepared = prepared_context(turn);
    let delivered: Vec<Value> = prepared["delivered"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    let faculty = match &request.faculty_evidence {
        Some(root) => match faculty_receipts(root, request.agent_session.as_str()) {
            Ok(receipts) => {
                absences.push("owner faculty receipts are read by schema and session; their content-addressed file names are not re-verified here".into());
                Some(receipts)
            }
            Err(e) => {
                absences.push(format!("faculty evidence unreadable: {}", e.message()));
                None
            }
        },
        None => {
            absences.push("no faculty evidence directory given, so owner receipts for Python faculty calls were not read".into());
            None
        }
    };
    let qlops = ql_operations(
        turn,
        faculty.as_deref(),
        (started_ms.unwrap_or(0), next_start_ms),
    );

    let redis = match &request.redis_config {
        Some(path) => redis_reading(path, &delivered),
        None => {
            absences.push("no Redis config given, so the delivered view's selected sources were not read back".into());
            json!({"read": false, "reason": "not requested"})
        }
    };
    let last_receipt = delivered.last().map(|e| &e["receipt"]);
    let decision = json!({
        "from_delivery_receipt": last_receipt.map(|r| json!({
            "decision_provider": r.get("decision_provider").cloned().unwrap_or(Value::Null),
            "jev_invocation_ref": r.get("jev_invocation_ref").cloned().unwrap_or(Value::Null),
        })),
        "selection_readback": redis,
        "note": "the prepare path returns the decision receipt but does not persist its body: only the invocation ref and the provider identity digest travel with the view and its delivery receipt",
    });
    if let Some(receipt) = last_receipt {
        if receipt.get("decision_provider").is_none_or(Value::is_null) {
            absences.push("the delivered view records no decision provider: its selection did not use one (or the receipt predates the field)".into());
        }
    } else {
        absences.push(
            "no prepared context was delivered to this turn, so there is no decision behind it"
                .into(),
        );
    }
    if turn.index == all.len() && outcome(turn) == "open" {
        absences.push(
            "the turn has no completion signal yet; the reading covers what was journalled so far"
                .into(),
        );
    }
    absences.push(
        "addressed deliveries are not segmented as turns; only user messages open a turn".into(),
    );

    Ok(json!({
        "schema": READING_SCHEMA,
        "agent_session": request.agent_session,
        "turn": {
            "index": turn.index, "of": all.len(),
            "start_cursor": turn.start,
            "end_cursor_exclusive": if turn.end == u64::MAX { Value::Null } else { json!(turn.end) },
            "started_at_ms": started_ms, "last_event_at_ms": ended_ms,
            "outcome": outcome(turn),
        },
        "prepared_context": prepared,
        "decision": decision,
        "ql_operations": qlops,
        "absences": absences,
        "sources": ["encounter journal (encounters.sqlite3)", "Actuation faculty evidence (actuation.prime-ql-operation/v1)", "Redis prepared view"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/proof").unwrap()
    }

    fn user(cursor: u64, at: u64) -> (u64, Value) {
        (
            cursor,
            json!({"kind": "user-message", "text": "q", "observed_at_ms": at}),
        )
    }
    fn signal(cursor: u64, at: u64, kind: Value) -> (u64, Value) {
        (
            cursor,
            json!({"kind": "provider", "observed_at_ms": at,
                   "event": {"Signal": {"kind": kind, "sequence": cursor}}}),
        )
    }
    fn call(id: &str, tool: &str, args: Value) -> Value {
        json!({"kind": "tool-call", "payload": {"toolName": tool, "toolCallId": id, "args": args}})
    }
    fn result(id: &str, tool: &str, text: &str) -> Value {
        json!({"kind": "tool-result", "payload": {"toolName": tool, "toolCallId": id, "isError": false,
               "result": {"content": [{"type": "text", "text": text}]}}})
    }
    fn receipt(version: u64) -> Value {
        json!({"schema": "aikit.now-context-delivery/v1", "participant_ref": "agent/a",
               "agent_session": "agent-session/proof", "prepared_version": version,
               "prepared_digest": "blake3:p", "basis_digest": "blake3:b", "change_cursor": 2,
               "delivered_at_unix_ms": 1100,
               "decision_provider": "blake3:kev", "jev_invocation_ref": "invocation/jev/abc"})
    }
    fn faculty(trace: &str, op: &str, at_ms: u64) -> Value {
        json!({"schema": "actuation.prime-ql-operation/v1", "operation": op, "trace_ref": trace,
               "observed_unix_nanos": (u128::from(at_ms) * 1_000_000).to_string(),
               "ql_mef_revision": "r", "request_digest": "q", "response_digest": "s",
               "success": true, "error": null, "declared_locus_ref": "locus"})
    }
    fn request(turn: TurnSelector) -> UseRequest {
        UseRequest {
            agent_session: session(),
            turn,
            faculty_evidence: None,
            redis_config: None,
        }
    }

    fn journal_fixture() -> Vec<(u64, Value)> {
        vec![
            // turn 1: nothing prepared, no QL
            user(1, 1000),
            signal(2, 1001, json!({"kind": "completed", "stop_reason": "stop"})),
            // turn 2: prepared context delivered, two QL tool calls, one Python faculty call
            user(3, 2000),
            (
                4,
                json!({"kind": "now-context-delivered", "receipt": receipt(3), "observed_at_ms": 2001}),
            ),
            signal(5, 2002, call("c1", "ql_project_event", json!({}))),
            signal(6, 2003, call("c1", "ql_project_event", json!({}))), // an update of the same call
            signal(
                7,
                2004,
                result(
                    "c1",
                    "ql_project_event",
                    r#"{"schema":"ql.agent-projection/v1"}"#,
                ),
            ),
            signal(
                8,
                2005,
                call(
                    "c2",
                    "ql_invoke",
                    json!({"operation": "anuttara-read", "position": "#0"}),
                ),
            ),
            signal(
                9,
                2006,
                call(
                    "py1",
                    "ipython",
                    json!({"code": "r = await ql_relational.mef_lenses()\nawait ql_relational.vak_locate('x')"}),
                ),
            ),
            signal(
                10,
                2007,
                json!({"kind": "completed", "stop_reason": "stop"}),
            ),
        ]
    }

    #[test]
    fn a_turn_reports_what_was_delivered_and_what_the_body_did() {
        let events = journal_fixture();
        let reading = assemble(&events, &request(TurnSelector::Last)).unwrap();
        assert_eq!(reading["turn"]["index"], 2);
        assert_eq!(reading["turn"]["outcome"], "completed");
        assert_eq!(reading["prepared_context"]["state"], "delivered");
        let delivered = &reading["prepared_context"]["delivered"][0];
        assert_eq!(delivered["receipt"]["prepared_version"], 3);
        assert_eq!(
            reading["decision"]["from_delivery_receipt"]["jev_invocation_ref"],
            "invocation/jev/abc"
        );
        let calls = reading["ql_operations"]["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2, "an updated call is one call");
        assert_eq!(calls[0]["tool"], "ql_project_event");
        assert_eq!(
            calls[0]["result"]["result_schema"],
            "ql.agent-projection/v1"
        );
        assert_eq!(calls[1]["operation"], "anuttara-read");
        assert_eq!(
            calls[1]["result"]["observed"], false,
            "a call with no recorded result says so"
        );
        let python: Vec<&str> = reading["ql_operations"]["python_faculty_calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["function"].as_str().unwrap())
            .collect();
        assert_eq!(python, ["mef_lenses", "vak_locate"]);
    }

    #[test]
    fn a_turn_with_no_delivery_says_none_and_names_why_there_is_no_decision() {
        let reading = assemble(&journal_fixture(), &request(TurnSelector::Index(1))).unwrap();
        assert_eq!(
            reading["prepared_context"]["state"],
            "none-selected-or-none-delivered"
        );
        assert!(reading["decision"]["from_delivery_receipt"].is_null());
        assert!(reading["absences"]
            .to_string()
            .contains("no prepared context was delivered"));
        assert_eq!(
            reading["ql_operations"]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    #[test]
    fn owner_receipts_are_matched_to_the_turn_by_session_and_time_and_reconciled_by_count() {
        let events = journal_fixture();
        let receipts = [
            faculty("agent-session/proof", "mef-lenses", 2006),
            faculty("agent-session/proof", "vak-locate", 2007),
            faculty("agent-session/proof", "old", 1001), // turn 1
            faculty("agent-session/other", "elsewhere", 2006), // another session
        ];
        // The filtering by trace happens when receipts are read; here the
        // time window decides.
        let qlops = ql_operations(&turns(&events)[1], Some(&receipts[..3]), (2000, None));
        let in_turn = qlops["faculty_receipts"].as_array().unwrap();
        assert_eq!(in_turn.len(), 2);
        assert!(qlops["reconciliation"]["statement"]
            .as_str()
            .unwrap()
            .contains("counts agree"));
        // A receipt missing for one call is stated, not hidden.
        let qlops = ql_operations(&turns(&events)[1], Some(&receipts[..1]), (2000, None));
        assert!(qlops["reconciliation"]["statement"]
            .as_str()
            .unwrap()
            .contains("differ"));
    }

    #[test]
    fn a_degraded_turn_is_degraded_not_delivered_and_an_unknown_turn_is_refused() {
        let events = vec![
            user(1, 1000),
            (
                2,
                json!({"kind": "now-context-degraded", "detail": {"code": "now_context.redis_io"}, "observed_at_ms": 1001}),
            ),
        ];
        let reading = assemble(&events, &request(TurnSelector::Last)).unwrap();
        assert_eq!(
            reading["prepared_context"]["state"],
            "degraded-none-delivered"
        );
        assert!(assemble(&events, &request(TurnSelector::Index(2))).is_err());
        assert!(assemble(&[], &request(TurnSelector::Last)).is_err());
        let by_cursor = assemble(&events, &request(TurnSelector::Cursor(2))).unwrap();
        assert_eq!(by_cursor["turn"]["index"], 1);
    }

    #[test]
    fn python_calls_are_extracted_from_code_not_guessed() {
        assert_eq!(
            python_faculty_calls("x = ql_relational.kernel_apply('a','b')\nprint('ql_relational is a module')\nql_relational.wiki_refract(r)"),
            ["kernel_apply", "wiki_refract"]
        );
        assert!(python_faculty_calls("import json").is_empty());
    }
}
