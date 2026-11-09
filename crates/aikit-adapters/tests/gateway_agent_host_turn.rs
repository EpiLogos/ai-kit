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

use aikit_adapters::connection_process::ModelEnvironment;
use aikit_adapters::{
    AgentHostTurnSource, ConversationHarnessProtocol, ConversationTurn, ConversationTurnOutcome,
    ConversationTurnRequest, ConversationTurnSource, HARNESS_NOT_CONFINED,
    TOOL_LESS_TURN_USED_A_TOOL,
};
use aikit_core::credential::SecretValue;
use aikit_core::resource::ResourceRef;
use tempfile::TempDir;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

const FIXTURE: &str = r#"
import json, sys, threading, time

counter = 0
cancel = {}
TOOLS = json.loads(sys.argv[1]) if len(sys.argv) > 1 else []
lock = threading.Lock()

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def run_turn(native, request_id, text):
    steps = 2 if "quick" in text else 8
    pause = 0.0 if "quick" in text else 0.25
    stop = "end_turn"
    if "tell your env" in text:
        import os
        listing = os.listdir(os.environ.get("HOME", "/nonexistent")) if os.path.isdir(os.environ.get("HOME", "")) else None
        said = json.dumps({"env": sorted(os.environ), "home": os.environ.get("HOME"), "cwd": os.getcwd(),
                           "home_listing": listing, "credential": os.environ.get("ANTHROPIC_API_KEY")})
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": native,
              "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": said}}}})
        send({"jsonrpc": "2.0", "id": request_id, "result": {"stopReason": stop}})
        return
    for kind in ("tool_result", "mcp_tool_progress"):
        if "send a " + kind in text:
            send({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": native, "update": {"sessionUpdate": kind, "toolCallId": "read-1",
                                                "content": [{"type": "text", "text": "/etc/hosts"}]}}})
    if "use a tool" in text:
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": native,
            "update": {"sessionUpdate": "tool_call", "toolCallId": "read-1", "title": "Read",
                       "kind": "read", "status": "pending", "rawInput": {"path": "/etc/hosts"}}}})
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
        result = {"sessionId": native}
        if TOOLS is not None:
            result["tools"] = TOOLS
        send({"jsonrpc": "2.0", "id": message["id"], "result": result})
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
    source_reporting("[]")
}

/// The fixture, its session reporting `tools` (JSON; `null` reports no list).
fn source_reporting(tools: &str) -> Option<(AgentHostTurnSource, TempDir)> {
    let dir = TempDir::new().unwrap();
    let mut argv = vec![
        python3()?,
        "-u".to_string(),
        "-c".to_string(),
        FIXTURE.to_string(),
    ];
    argv.push(tools.to_owned());
    let source = AgentHostTurnSource::new(
        "acp-fixture",
        ConversationHarnessProtocol::Acp,
        argv,
        dir.path().join("work"),
    );
    std::fs::create_dir_all(dir.path().join("work")).unwrap();
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

/// python3 by its full path: a confined launch passes PATH on, but the
/// fixture is named whole so the proof does not lean on it.
fn python3() -> Option<String> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("python3"))
            .find(|candidate| candidate.is_file())
            .map(|found| found.display().to_string())
    })
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

fn ended(turn: &dyn ConversationTurn) -> ConversationTurnOutcome {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match turn.wait_timeout(Duration::from_millis(250)) {
            Some(outcome) => return outcome,
            None if Instant::now() > deadline => panic!("the turn never ended"),
            None => continue,
        }
    }
}

/// A source whose turns take no tool (ai-kit #601: a World's agent answering
/// an ask from what its World shows): a harness that calls a tool all the same
/// is stopped at that call, and the turn ends failed, so nothing it said after
/// a tool is taken for its answer. A turn that calls none replies as ever, and
/// an ordinary source still carries its tool calls.
#[test]
fn a_tool_less_turn_that_calls_a_tool_is_stopped_and_answers_nothing() {
    let Some((held, _dir)) = source() else {
        return;
    };
    let tool_less = held.without_tools();
    let turn = tool_less.prompt(request("quick: use a tool")).unwrap();
    assert_eq!(
        ended(turn.as_ref()),
        ConversationTurnOutcome::Failed {
            reason: TOOL_LESS_TURN_USED_A_TOOL.into()
        },
        "a tool call ends the turn failed, whatever the harness said after it"
    );
    let next = tool_less.prompt(request("quick and plain")).unwrap();
    let ConversationTurnOutcome::Replied { text } = ended(next.as_ref()) else {
        panic!("a turn that calls no tool replies");
    };
    assert!(text.contains("quick and plain"), "{text}");

    let Some((ordinary, _other)) = source() else {
        return;
    };
    let turn = ordinary.prompt(request("quick: use a tool")).unwrap();
    assert!(
        matches!(
            ended(turn.as_ref()),
            ConversationTurnOutcome::Replied { .. }
        ),
        "an ordinary source's turns may call tools"
    );
}

/// Confined before the prompt (DLW build ruling 2026-11-09): a tool-less
/// turn is prompted only when the session it opened reports an empty tool
/// list. A session that reports tools, or (on a protocol that reports them)
/// no list at all, is refused before anything is prompted, in words naming
/// the harness.
#[test]
fn a_tool_less_turn_is_refused_before_the_prompt_unless_its_session_reports_no_tools() {
    for (tools, why) in [
        (
            r#"["Read", {"name": "Bash"}]"#,
            "reports the tools Read, Bash",
        ),
        ("null", "reports no tool list"),
    ] {
        let Some((source, _dir)) = source_reporting(tools) else {
            return;
        };
        let error = source
            .without_tools()
            .prompt(request("quick and plain"))
            .err()
            .unwrap_or_else(|| panic!("a session that {why} is not prompted"));
        assert_eq!(error.code(), HARNESS_NOT_CONFINED, "{why}");
        assert!(error.to_string().contains("acp-fixture"), "{error}");
        assert!(error.to_string().contains(why), "{error}");
    }
    let Some((source, _dir)) = source_reporting("[]") else {
        return;
    };
    // The source is held while its turn runs: dropping it stops its host.
    let source = source.without_tools();
    let turn = source.prompt(request("quick and plain")).unwrap();
    assert!(matches!(
        ended(turn.as_ref()),
        ConversationTurnOutcome::Replied { .. }
    ));

    // pi is held to no tools by its launch flags alone: an argv without them
    // is refused before anything is launched.
    let dir = TempDir::new().unwrap();
    let pi = |argv: &[&str]| {
        AgentHostTurnSource::new(
            "pi",
            ConversationHarnessProtocol::PiRpc,
            argv.iter().map(|arg| arg.to_string()).collect(),
            dir.path().to_path_buf(),
        )
        .without_tools()
        .prompt(request("quick"))
        .err()
        .expect("refused before launch")
    };
    let error = pi(&["/nonexistent/pi", "--mode", "rpc", "--no-tools"]);
    assert_eq!(error.code(), HARNESS_NOT_CONFINED);
    assert!(error.to_string().contains("--no-extensions"), "{error}");
    let error = pi(&[
        "/nonexistent/pi",
        "--mode",
        "rpc",
        "-nt",
        "-ne",
        "-ns",
        "--no-mcp",
        "-nc",
        "--tools",
        "read",
    ]);
    assert_eq!(error.code(), HARNESS_NOT_CONFINED);
    assert!(error.to_string().contains("--tools"), "{error}");
    assert!(ConversationHarnessProtocol::PiRpc
        .confines(&["pi", "-nt", "-ne", "-ns", "--no-mcp", "-nc"].map(String::from))
        .is_ok());
}

/// The confined launch's environment: PATH and LANG, the provider's own
/// credential variable, and an empty HOME of its own beside its empty
/// working folder; nothing else of this process (no OI_*, no AIKit
/// variable, no cargo environment) reaches the harness.
#[test]
fn a_confined_launch_sees_only_path_lang_its_credential_and_an_empty_home() {
    let Some((source, dir)) = source() else {
        return;
    };
    let work = dir.path().join("work");
    let home = dir.path().join("work.home");
    let environment = ModelEnvironment::isolated(&home)
        .with_credential(
            "ANTHROPIC_API_KEY",
            SecretValue::new("fixture-credential").unwrap(),
        )
        .unwrap();
    let source = source.without_tools().with_launch_environment(environment);
    let turn = source.prompt(request("tell your env")).unwrap();
    let ConversationTurnOutcome::Replied { text } = ended(turn.as_ref()) else {
        panic!("the turn replies");
    };
    let said: serde_json::Value = serde_json::from_str(&text).unwrap();
    let allowed = [
        "PATH",
        "LANG",
        "HOME",
        "ANTHROPIC_API_KEY",
        "LC_CTYPE",
        "__CF_USER_TEXT_ENCODING",
    ];
    for name in said["env"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert!(
            allowed.contains(&name),
            "{name} reached a confined harness: {said}"
        );
    }
    assert_eq!(said["home"], home.display().to_string());
    assert_eq!(
        said["home_listing"],
        serde_json::json!([]),
        "its HOME is empty"
    );
    assert_eq!(
        std::fs::canonicalize(said["cwd"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&work).unwrap()
    );
    assert_eq!(said["credential"], "fixture-credential");

    // The default confinement: the same, with no credential at all.
    let Some((source, dir)) = source_reporting("[]") else {
        return;
    };
    let source = source.without_tools();
    let turn = source.prompt(request("tell your env")).unwrap();
    let ConversationTurnOutcome::Replied { text } = ended(turn.as_ref()) else {
        panic!("the turn replies");
    };
    let said: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        said["home"],
        dir.path().join("work.home").display().to_string()
    );
    assert!(said["credential"].is_null());
    assert!(
        said["env"].as_array().unwrap().len() < allowed.len(),
        "{said}"
    );
}

/// The guard is the backstop on every tool event: a tool's result, or a
/// provider update about a tool, ends a tool-less turn as one that used a
/// tool, whatever it said after.
#[test]
fn a_tool_result_or_a_tool_update_ends_a_tool_less_turn() {
    for kind in ["tool_result", "mcp_tool_progress"] {
        let Some((source, _dir)) = source() else {
            return;
        };
        let source = source.without_tools();
        let turn = source
            .prompt(request(&format!("quick: send a {kind}")))
            .unwrap();
        assert_eq!(
            ended(turn.as_ref()),
            ConversationTurnOutcome::Failed {
                reason: TOOL_LESS_TURN_USED_A_TOOL.into()
            },
            "{kind}"
        );
    }
}
