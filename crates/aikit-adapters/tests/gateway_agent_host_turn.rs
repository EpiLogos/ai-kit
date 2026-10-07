//! The gateway conversation engine's host-backed turn control, against a real
//! harness process: a `/stop`-class interrupt actually cancels the running
//! turn on the wire, the turn's own end records the interruption, and the
//! lane is free for the very next prompt — the exact flow a conversation
//! surface needs when the person stops a turn and speaks again.
//!
//! The fixture is a stdlib-only Python ACP agent whose slow turns honour a
//! cancel at the next chunk boundary, the smallest real thing that can prove
//! the cancel reaches the harness and the lane really frees.

use std::time::{Duration, Instant};

use aikit_adapters::{
    AgentHostTurnSource, ConversationHarnessProtocol, ConversationTurnOutcome,
    ConversationTurnRequest, ConversationTurnSource,
};
use aikit_core::resource::ResourceRef;
use tempfile::TempDir;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

const FIXTURE: &str = r#"
import json, sys, threading, time

counter = 0
cancel = {}
lock = threading.Lock()

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def run_turn(native, request_id, text):
    steps = 2 if "quick" in text else 8
    pause = 0.0 if "quick" in text else 0.25
    stop = "end_turn"
    for index in range(steps):
        with lock:
            if cancel.get(native):
                stop = "cancelled"
                break
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": native,
            "update": {"sessionUpdate": "agent_message_chunk",
                       "content": {"type": "text", "text": "%s#%d:%s" % (native, index, text)}}}})
        if pause:
            time.sleep(pause)
    send({"jsonrpc": "2.0", "id": request_id, "result": {"stopReason": stop}})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": message["id"], "result": {
            "protocolVersion": 1,
            "agentCapabilities": {"loadSession": True,
                                  "sessionCapabilities": {"resume": {}}}}})
    elif method in ("session/new", "session/load", "session/resume"):
        with lock:
            counter += 1
            native = "native-%d" % counter
        send({"jsonrpc": "2.0", "id": message["id"], "result": {"sessionId": native}})
    elif method == "session/prompt":
        native = message["params"]["sessionId"]
        text = "".join(
            part.get("text", "") if isinstance(part, dict) else str(part)
            for part in message["params"]["prompt"]
        )
        with lock:
            cancel[native] = False
        threading.Thread(target=run_turn, args=(native, message["id"], text), daemon=True).start()
    elif method == "session/cancel":
        native = message["params"]["sessionId"]
        with lock:
            cancel[native] = True
"#;

fn source() -> Option<(AgentHostTurnSource, TempDir)> {
    let dir = TempDir::new().unwrap();
    let argv = vec![
        "python3".to_string(),
        "-u".to_string(),
        "-c".to_string(),
        FIXTURE.to_string(),
    ];
    let source = AgentHostTurnSource::new(
        "acp-fixture",
        ConversationHarnessProtocol::Acp,
        argv,
        dir.path().to_path_buf(),
    );
    match source.sessions() {
        Ok(_) => Some((source, dir)),
        Err(error) if error.code() == "connection.process.spawn_failed" => {
            eprintln!("SKIP gateway_agent_host_turn: python3 is unavailable for the fixture");
            None
        }
        // `sessions()` opens the host lazily; a spawn failure surfaces there.
        Err(error) if error.code() == "agent_session_host.launch_failed" => {
            eprintln!("SKIP gateway_agent_host_turn: python3 is unavailable for the fixture");
            None
        }
        Err(error) => panic!("unexpected turn source failure: {error}"),
    }
}

fn request(prompt: &str) -> ConversationTurnRequest {
    ConversationTurnRequest {
        binding_ref: r("gateway-binding/gateway-host-turn"),
        agent_session_ref: r("agent-session/gateway-host-turn"),
        prompt: prompt.to_owned(),
        in_reply_to_sequence: 1,
    }
}

#[test]
fn stop_actually_cancels_the_harness_turn_and_the_lane_frees_for_the_next_prompt() {
    let Some((source, _dir)) = source() else {
        return;
    };

    // A slow turn starts; a stop is requested while it runs.
    let turn = source.prompt(request("a slow turn to interrupt")).unwrap();
    let receipt = turn
        .interrupt(Some("the person changed course".into()))
        .expect("the interrupt reaches the harness through the host's own seam");
    assert!(
        receipt.contains("session/cancel"),
        "the receipt names the cancel the harness was sent: {receipt}"
    );

    // The turn ends because the harness honoured the cancel — the outcome is
    // the turn's own interruption, never an invented one — and the wait
    // returns only after the harness answered it.
    let deadline = Instant::now() + Duration::from_secs(30);
    let outcome = loop {
        match turn.wait_timeout(Duration::from_millis(250)) {
            Some(outcome) => break outcome,
            None if Instant::now() > deadline => {
                panic!("the interrupted turn never ended")
            }
            None => continue,
        }
    };
    assert!(
        matches!(outcome, ConversationTurnOutcome::Interrupted { .. }),
        "the turn ended as the interruption it was: {outcome:?}"
    );

    // The lane is free: the conversation's very next message starts a fresh
    // turn instead of meeting the one-turn-at-a-time guard.
    let next = source
        .prompt(request("a quick fresh question"))
        .expect("the freed lane accepts the next prompt");
    let outcome = loop {
        match next.wait_timeout(Duration::from_millis(250)) {
            Some(outcome) => break outcome,
            None if Instant::now() > deadline => {
                panic!("the fresh turn never ended")
            }
            None => continue,
        }
    };
    let ConversationTurnOutcome::Replied { text } = outcome else {
        panic!("the fresh turn replied: {outcome:?}");
    };
    assert!(
        text.contains("a quick fresh question"),
        "the reply is the fresh turn's own answer: {text}"
    );

    // A stop asked of a turn that already ended is answered honestly, not
    // failed: there was simply nothing left to cancel.
    let finished_receipt = next
        .interrupt(Some("late stop".into()))
        .expect("an interrupt of a finished turn answers, it does not fail");
    assert!(
        finished_receipt.contains("already ended"),
        "the finished turn says so: {finished_receipt}"
    );
}
