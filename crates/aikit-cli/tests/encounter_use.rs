//! The actual-use reading through the real `aikit-session-space` binary over a
//! real encounter journal (`EncounterStore`) and a real faculty-receipt
//! directory, for a turn in which the body was delivered prepared context and
//! made QL calls — and for one in which it was not.
//!
//! The journal is written through the store's own `append` with the event
//! shapes the encounter owner records; no provider or model is involved, so
//! this pins the *reading*, not that a live body called QL (the real-Redis
//! delivery path is covered in `now_context_encounter`).
#![cfg(unix)]

use aikit_core::ResourceRef;
use aikit_store::{encounter::EncounterStore, AikitHome};
use serde_json::{json, Value};
use std::{fs, process::Command};
use tempfile::TempDir;

fn signal(kind: Value) -> Value {
    json!({"kind": "provider", "event": {"Signal": {"kind": kind, "sequence": 1}}})
}

#[test]
fn a_turn_reads_back_delivery_decision_and_ql_operations_and_says_what_is_absent() {
    let root = TempDir::new().unwrap();
    let home = AikitHome::at(root.path().join("aikit"));
    let session = ResourceRef::parse("agent-session/use-proof").unwrap();
    let store = EncounterStore::open(&home).unwrap();

    // Turn 1: an ordinary turn.
    store
        .append(&session, &json!({"kind": "user-message", "text": "hello"}))
        .unwrap();
    store
        .append(
            &session,
            &signal(json!({"kind": "completed", "stop_reason": "stop"})),
        )
        .unwrap();

    // Turn 2: prepared context delivered, QL calls made.
    store
        .append(
            &session,
            &json!({"kind": "user-message", "text": "ground the question"}),
        )
        .unwrap();
    store
        .append(
            &session,
            &json!({"kind": "now-context-delivered", "receipt": {
                "schema": "aikit.now-context-delivery/v1", "participant_ref": "agent/use-proof",
                "agent_session": "agent-session/use-proof", "prepared_version": 4,
                "prepared_digest": "blake3:p4", "basis_digest": "blake3:b4", "change_cursor": 7,
                "delivered_at_unix_ms": 1,
                "decision_provider": "blake3:kev-identity",
                "jev_invocation_ref": "invocation/jev/kev-1"}}),
        )
        .unwrap();
    let call = |id: &str, tool: &str, args: Value| {
        signal(
            json!({"kind": "tool-call", "payload": {"toolName": tool, "toolCallId": id, "args": args}}),
        )
    };
    store
        .append(&session, &call("t1", "ql_project_event", json!({})))
        .unwrap();
    store
        .append(
            &session,
            &signal(json!({"kind": "tool-result", "payload": {"toolName": "ql_project_event",
                "toolCallId": "t1", "isError": false,
                "result": {"content": [{"type": "text", "text": "{\"schema\":\"ql.agent-projection/v1\"}"}]}}})),
        )
        .unwrap();
    store
        .append(
            &session,
            &call(
                "p1",
                "ipython",
                json!({"code": "await ql_relational.mef_lenses()"}),
            ),
        )
        .unwrap();
    // The owner's own receipt for that Python call, written now so it lies in the turn.
    let evidence = root.path().join("faculty-evidence");
    fs::create_dir_all(&evidence).unwrap();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_string();
    for (name, trace) in [
        ("mine", "agent-session/use-proof"),
        ("other", "agent-session/someone-else"),
    ] {
        fs::write(
            evidence.join(format!("{name}.json")),
            serde_json::to_vec(&json!({
                "schema": "actuation.prime-ql-operation/v1", "operation": "mef-lenses",
                "trace_ref": trace, "observed_unix_nanos": nanos, "ql_mef_revision": "rev",
                "request_digest": "q", "response_digest": "s", "success": true, "error": null,
                "declared_locus_ref": "locus"
            }))
            .unwrap(),
        )
        .unwrap();
    }
    store
        .append(
            &session,
            &signal(json!({"kind": "completed", "stop_reason": "stop"})),
        )
        .unwrap();

    let read = |extra: &[&str]| -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root.path())
            .env("AIKIT_HOME", home.root())
            .arg("-C")
            .arg(root.path())
            .args([
                "encounter-use",
                "--agent-session",
                "agent-session/use-proof",
            ])
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    };

    // The last turn, with the faculty evidence directory.
    let reading = read(&["--faculty-evidence", evidence.to_str().unwrap()]);
    assert_eq!(reading["schema"], "aikit.encounter-use-reading/v1");
    assert_eq!(reading["turn"]["index"], 2);
    assert_eq!(reading["turn"]["of"], 2);
    assert_eq!(reading["turn"]["outcome"], "completed");
    assert_eq!(reading["prepared_context"]["state"], "delivered");
    assert_eq!(
        reading["prepared_context"]["delivered"][0]["receipt"]["prepared_version"],
        4
    );
    assert_eq!(
        reading["decision"]["from_delivery_receipt"],
        json!({"decision_provider": "blake3:kev-identity", "jev_invocation_ref": "invocation/jev/kev-1"})
    );
    let calls = reading["ql_operations"]["tool_calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["tool"], "ql_project_event");
    assert_eq!(
        calls[0]["result"]["result_schema"],
        "ql.agent-projection/v1"
    );
    assert_eq!(
        reading["ql_operations"]["python_faculty_calls"][0]["function"],
        "mef_lenses"
    );
    let receipts = reading["ql_operations"]["faculty_receipts"]
        .as_array()
        .unwrap();
    assert_eq!(
        receipts.len(),
        1,
        "only this session's receipt, not another's"
    );
    assert_eq!(receipts[0]["operation"], "mef-lenses");
    assert!(reading["ql_operations"]["reconciliation"]["statement"]
        .as_str()
        .unwrap()
        .contains("counts agree"));
    // What was not read is stated: no Redis config was given.
    assert!(reading["absences"]
        .to_string()
        .contains("no Redis config given"));

    // Turn 1 delivered nothing and made no QL call; the reading says so.
    let first = read(&["--turn", "1"]);
    assert_eq!(
        first["prepared_context"]["state"],
        "none-selected-or-none-delivered"
    );
    assert!(first["decision"]["from_delivery_receipt"].is_null());
    assert_eq!(
        first["ql_operations"]["tool_calls"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(first["absences"]
        .to_string()
        .contains("no faculty evidence directory"));

    // A turn that does not exist is refused, not invented.
    let output = Command::new(env!("CARGO_BIN_EXE_aikit-session-space"))
        .env("AIKIT_HOME", home.root())
        .arg("-C")
        .arg(root.path())
        .args([
            "encounter-use",
            "--agent-session",
            "agent-session/use-proof",
            "--turn",
            "9",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no such turn"));
}
