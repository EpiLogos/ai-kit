//! Real-process conformance for `aikit.agent-session-host/v1`.
//!
//! The fixture is a stdlib-only Python ACP agent that carries several native
//! sessions on one stdio transport and streams them at different cadences. That
//! is the smallest real thing that can prove the three properties the host
//! exists for: streaming in wire order, sessions that do not block one another
//! on one transport, and a mid-turn interrupt that stops the turn while the
//! canonical AgentSession identity stays exactly what the caller supplied.

use std::time::{Duration, Instant};

use aikit_adapters::{
    AcpStableConnectionAdapter, AgentSessionHost, AgentSessionHostLimits, ConnectionSignalKind,
    HostEvent, InterruptOrigin, SessionLane, SessionLaneState, SessionOpenMode, SessionOpenRequest,
    TurnStop, DEFAULT_MAX_SIGNALS_PER_TURN,
};
use aikit_core::resource::ResourceRef;
use serde_json::json;

fn r(raw: &str) -> ResourceRef {
    ResourceRef::parse(raw).unwrap()
}

const FIXTURE: &str = r#"
import json, os, sys, threading, time

counter = 0
turns = {}
lock = threading.Lock()

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def text_of(prompt):
    parts = prompt if isinstance(prompt, list) else [prompt]
    out = []
    for part in parts:
        if isinstance(part, dict) and part.get("type") == "text":
            out.append(part.get("text", ""))
        elif isinstance(part, str):
            out.append(part)
    return "".join(out)

def cancelled(native):
    with lock:
        return turns.get(native, {}).get("cancel", False)

def run_turn(native, request_id, text):
    slow = "slow" in text
    chunks = 3 if slow else 1
    # A turn that finishes on its own even though a cancel arrived: the host's
    # interrupt lost the race, and must not record the turn as interrupted.
    ignore_cancel = "raceignore" in text
    for index in range(chunks):
        if cancelled(native) and not ignore_cancel:
            break
        send({"jsonrpc": "2.0", "method": "session/update", "params": {
            "sessionId": native,
            "update": {"sessionUpdate": "agent_message_chunk",
                       "content": {"type": "text", "text": "%s#%d:%s" % (native, index, text)}}}})
        if "die" in text:
            sys.stdout.flush()
            os._exit(0)
        if slow:
            time.sleep(0.35)
    if (cancelled(native) and not ignore_cancel) or "selfcancel" in text:
        stop = "cancelled"
    else:
        stop = "end_turn"
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
        text = text_of(message["params"]["prompt"])
        with lock:
            turns[native] = {"cancel": False}
        if "orphan" in text:
            send({"jsonrpc": "2.0", "method": "session/update", "params": {
                "sessionId": "native-orphan",
                "update": {"sessionUpdate": "agent_message_chunk",
                           "content": {"type": "text", "text": "not attributed"}}}})
        threading.Thread(target=run_turn, args=(native, message["id"], text), daemon=True).start()
    elif method == "session/cancel":
        native = message["params"]["sessionId"]
        with lock:
            turns.setdefault(native, {})["cancel"] = True
"#;

fn launch() -> Option<AgentSessionHost> {
    let argv = vec![
        "python3".to_string(),
        "-u".to_string(),
        "-c".to_string(),
        FIXTURE.to_string(),
    ];
    let launched = AgentSessionHost::launch(
        AcpStableConnectionAdapter::new(
            r("connection/acp/session-host-fixture"),
            vec!["stdlib ACP fixture: several native sessions on one stdio transport".into()],
        ),
        &argv,
        None,
        AgentSessionHostLimits::default(),
    );
    let host = match launched {
        Ok(host) => host,
        Err(error) if error.code() == "connection.process.spawn_failed" => {
            eprintln!("SKIP agent_session_host_v2: python3 is unavailable for the stdio fixture");
            return None;
        }
        Err(error) => panic!("unexpected launch failure: {error}"),
    };
    host.initialize().unwrap();
    Some(host)
}

fn open(host: &AgentSessionHost, canonical: &str) -> SessionLane {
    host.open_session(SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd: std::env::current_dir().unwrap().display().to_string(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        agent_session: Some(r(canonical)),
    })
    .unwrap()
}

fn chunk_text(event: &HostEvent) -> Option<&str> {
    match event {
        HostEvent::Signal(signal) => match &signal.kind {
            ConnectionSignalKind::AgentMessageChunk { text } => Some(text),
            _ => None,
        },
        HostEvent::TurnEnded(_) => None,
    }
}

#[test]
fn acp_host_negotiates_then_streams_one_turn_in_wire_order() {
    let Some(host) = launch() else {
        return;
    };
    let capabilities = host.descriptor().unwrap().capabilities;
    assert!(capabilities.supports(SessionOpenMode::Create));
    assert!(capabilities.supports(SessionOpenMode::Resume));
    assert!(capabilities.ordered_streaming);

    let canonical = r("agent-session/host-streaming");
    let lane = open(&host, "agent-session/host-streaming");
    assert_eq!(lane.agent_session(), &canonical);
    assert_eq!(lane.binding().agent_session.as_ref(), Some(&canonical));
    assert!(!lane.binding().native_session_id.is_empty());

    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow streaming turn" }]))
        .unwrap();
    let mut chunks = Vec::new();
    let mut sequences = Vec::new();
    let record;
    loop {
        match turn.recv().unwrap() {
            HostEvent::Signal(signal) => {
                sequences.push(signal.sequence);
                if let ConnectionSignalKind::AgentMessageChunk { text } = &signal.kind {
                    chunks.push(text.clone());
                }
            }
            HostEvent::TurnEnded(finished) => {
                record = finished;
                break;
            }
        }
    }
    assert_eq!(
        chunks.len(),
        3,
        "every streamed chunk arrives as its own signal, as the provider emitted it"
    );
    for (index, text) in chunks.iter().enumerate() {
        assert_eq!(
            text,
            &format!(
                "{}#{}:slow streaming turn",
                lane.binding().native_session_id,
                index
            )
        );
    }
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "signal sequences increase in observed wire order"
    );
    assert_eq!(
        record.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    assert_eq!(record.agent_session, canonical);
    assert_eq!(record.binding.agent_session.as_ref(), Some(&canonical));
    assert!(
        record.interruption.is_none(),
        "a completed turn records no interruption"
    );
    assert_eq!(record.signals, 4, "three chunks plus the terminal signal");
    assert_eq!(host.last_turn(&canonical).unwrap().as_ref(), Some(&record));
    assert_eq!(host.interruptions(&canonical).unwrap(), Vec::new());
    assert_eq!(
        host.identity(&canonical).unwrap().state,
        SessionLaneState::Resident
    );
    host.shutdown().unwrap();
}

#[test]
fn two_sessions_on_one_transport_stream_without_blocking_one_another() {
    let Some(host) = launch() else {
        return;
    };
    let left = r("agent-session/host-concurrent-left");
    let right = r("agent-session/host-concurrent-right");
    let lane_left = open(&host, "agent-session/host-concurrent-left");
    let lane_right = open(&host, "agent-session/host-concurrent-right");
    assert_ne!(
        lane_left.binding().native_session_id,
        lane_right.binding().native_session_id,
        "one native session per canonical identity, and no shared native id"
    );

    let turn_left = lane_left
        .prompt(json!([{ "type": "text", "text": "slow left turn" }]))
        .unwrap();
    // The left turn is in flight, the fixture asleep between chunks. The right
    // session must complete a whole turn without waiting for it.
    let first = turn_left.recv().unwrap();
    let first_text = chunk_text(&first)
        .expect("the left turn starts with a streamed chunk")
        .to_string();
    assert!(first_text.starts_with(lane_left.binding().native_session_id.as_str()));

    let started = Instant::now();
    let turn_right = lane_right
        .prompt(json!([{ "type": "text", "text": "fast right turn" }]))
        .unwrap();
    let record_right = turn_right.wait().unwrap();
    let right_elapsed = started.elapsed();
    assert_eq!(
        record_right.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    assert_eq!(record_right.agent_session, right);
    assert!(
        right_elapsed < Duration::from_millis(1500),
        "the right turn must not wait behind the left turn; it took {right_elapsed:?}"
    );
    assert_eq!(
        host.identity(&left).unwrap().state,
        SessionLaneState::TurnInFlight,
        "the left turn is still in flight after the right turn finished"
    );

    let record_left = turn_left.wait().unwrap();
    assert_eq!(
        record_left.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    assert_eq!(record_left.agent_session, left);
    assert_eq!(
        host.interruptions(&left).unwrap(),
        Vec::new(),
        "concurrency alone records no interruption"
    );
    assert!(host.unattributed().unwrap().is_empty());
    host.shutdown().unwrap();
}

#[test]
fn mid_turn_interrupt_stops_the_turn_and_preserves_canonical_identity() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-interrupt");
    let lane = open(&host, "agent-session/host-interrupt");
    let native = lane.binding().native_session_id.clone();

    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow turn to interrupt" }]))
        .unwrap();
    let first = turn.recv().unwrap();
    assert!(
        chunk_text(&first).is_some(),
        "interrupt after the turn started"
    );

    let receipt = turn.interrupt(Some("human changed course".into())).unwrap();
    assert_eq!(receipt.agent_session, canonical);
    assert_eq!(receipt.native_session_id, native);
    assert_eq!(receipt.commands, vec!["session/cancel".to_string()]);
    assert_eq!(receipt.reason.as_deref(), Some("human changed course"));
    assert_eq!(
        host.identity(&canonical).unwrap().state,
        SessionLaneState::InterruptRequested
    );

    let record = turn.wait().unwrap();
    assert_eq!(
        record.agent_session, canonical,
        "identity survives the interrupt"
    );
    assert_eq!(record.binding.agent_session.as_ref(), Some(&canonical));
    assert_eq!(record.binding.native_session_id, native);
    assert_eq!(record.stop, TurnStop::Cancelled);
    let interruption = record
        .interruption
        .as_ref()
        .expect("an interrupted turn records the interruption");
    assert_eq!(interruption.agent_session, canonical);
    assert_eq!(interruption.native_session_id, native);
    assert_eq!(interruption.origin, InterruptOrigin::Human);
    assert_eq!(interruption.reason.as_deref(), Some("human changed course"));
    assert_eq!(interruption.commands, vec!["session/cancel".to_string()]);
    assert_eq!(interruption.observed_stop, TurnStop::Cancelled);
    assert!(interruption.requested_at_sequence.is_some());

    let trail = host.interruptions(&canonical).unwrap();
    assert_eq!(
        trail.len(),
        1,
        "the interruption is recorded once on the trail"
    );
    assert_eq!(&trail[0], interruption);

    // The session is still resident, still itself, and carries another turn.
    assert_eq!(
        host.identity(&canonical).unwrap().binding.native_session_id,
        native
    );
    let next = lane
        .prompt(json!([{ "type": "text", "text": "fast after interrupt" }]))
        .unwrap();
    let next_record = next.wait().unwrap();
    assert_eq!(
        next_record.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    assert_eq!(next_record.binding.native_session_id, native);
    assert_eq!(
        host.interruptions(&canonical).unwrap().len(),
        1,
        "the turn after the interrupt records no interruption"
    );
    host.shutdown().unwrap();
}

#[test]
fn a_provider_cancelled_turn_is_recorded_as_provider_origin() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-provider-cancel");
    let lane = open(&host, "agent-session/host-provider-cancel");
    let native = lane.binding().native_session_id.clone();

    // The fixture cancels the turn on its own side: nobody on this host asked
    // for the stop, and the record has to say so rather than claim a human act.
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow selfcancel" }]))
        .unwrap();
    let record = turn.wait().unwrap();
    assert_eq!(record.stop, TurnStop::Cancelled);
    assert_eq!(record.agent_session, canonical);
    assert_eq!(record.binding.native_session_id, native);
    let interruption = record
        .interruption
        .as_ref()
        .expect("a cancelled turn records the interruption");
    assert_eq!(interruption.origin, InterruptOrigin::Provider);
    assert_eq!(interruption.commands, Vec::<String>::new());
    assert_eq!(interruption.requested_at_sequence, None);
    assert_eq!(interruption.reason, None);
    assert_eq!(interruption.observed_stop, TurnStop::Cancelled);
    assert_eq!(host.interruptions(&canonical).unwrap().len(), 1);
    host.shutdown().unwrap();
}

#[test]
fn the_host_refuses_to_invent_or_collapse_canonical_identity() {
    let Some(host) = launch() else {
        return;
    };

    // No canonical ref, no session: the host never synthesizes one from a
    // provider-native id.
    let unbound = host.open_session(SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd: std::env::current_dir().unwrap().display().to_string(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        agent_session: None,
    });
    assert_eq!(
        unbound.unwrap_err().code(),
        "agent_session_host.canonical_identity_required"
    );

    let lane = open(&host, "agent-session/host-identity");

    // A second native form of one canonical identity cannot be opened on this
    // host: that identity is already resident here.
    let duplicate = host.open_session(SessionOpenRequest {
        mode: SessionOpenMode::Create,
        native_session_id: None,
        cwd: std::env::current_dir().unwrap().display().to_string(),
        additional_directories: Vec::new(),
        mcp_servers: Vec::new(),
        agent_session: Some(r("agent-session/host-identity")),
    });
    assert_eq!(
        duplicate.unwrap_err().code(),
        "agent_session_host.canonical_session_already_open"
    );

    // Nothing is bound to a canonical ref the host never opened.
    let stranger = r("agent-session/host-never-opened");
    assert_eq!(
        host.identity(&stranger).unwrap_err().code(),
        "agent_session_host.session_not_open"
    );
    assert_eq!(host.interruptions(&stranger).unwrap(), Vec::new());
    assert_eq!(
        host.lane(&stranger).unwrap_err().code(),
        "agent_session_host.session_not_open"
    );

    // One session carries one turn at a time.
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow identity turn" }]))
        .unwrap();
    let _ = turn.recv().unwrap();
    let overlap = lane
        .prompt(json!([{ "type": "text", "text": "fast overlap" }]))
        .unwrap_err();
    assert_eq!(overlap.code(), "agent_session_host.turn_already_in_flight");
    turn.wait().unwrap();

    // Signals the host cannot attribute are recorded, never dropped silently.
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "orphan attribution" }]))
        .unwrap();
    turn.wait().unwrap();
    let unattributed = host.unattributed().unwrap();
    assert_eq!(unattributed.len(), 1);
    assert_eq!(
        unattributed[0].native_session_id.as_deref(),
        Some("native-orphan")
    );
    host.shutdown().unwrap();
}

#[test]
fn a_second_interrupt_and_an_interrupt_without_a_turn_are_refused_honestly() {
    let Some(host) = launch() else {
        return;
    };
    let lane = open(&host, "agent-session/host-interrupt-errors");

    assert_eq!(
        lane.interrupt(None).unwrap_err().code(),
        "agent_session_host.no_turn_in_flight"
    );

    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow double interrupt" }]))
        .unwrap();
    let _ = turn.recv().unwrap();
    turn.interrupt(Some("first".into())).unwrap();
    assert_eq!(
        turn.interrupt(Some("second".into())).unwrap_err().code(),
        "agent_session_host.interrupt_already_requested"
    );
    turn.wait().unwrap();

    assert_eq!(
        lane.interrupt(None).unwrap_err().code(),
        "agent_session_host.no_turn_in_flight"
    );
    host.shutdown().unwrap();
}

#[test]
fn shutdown_records_no_transport_failure_and_identity_is_untouched() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-shutdown");
    let lane = open(&host, "agent-session/host-shutdown");
    let native = lane.binding().native_session_id.clone();
    let transport_error = host.transport_error();

    let status = host.shutdown().unwrap();
    assert!(
        status.is_some(),
        "the fixture process was running and was stopped"
    );
    assert!(
        transport_error.is_none(),
        "a deliberate shutdown is not a transport failure"
    );
    // The canonical identity the caller supplied is not rewritten by the host.
    assert_eq!(lane.binding().agent_session.as_ref(), Some(&canonical));
    assert_eq!(lane.binding().native_session_id, native);
}

#[test]
fn a_transport_death_ends_its_turns_and_its_lanes() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-death");
    let lane = open(&host, "agent-session/host-death");

    // The fixture kills its own process mid-turn: the bridge cannot produce
    // anything after this, so the turn must end as failed and the lane must
    // end rather than block a reader forever.
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow die" }]))
        .unwrap();
    let record;
    loop {
        match turn.recv().unwrap() {
            HostEvent::Signal(_) => continue,
            HostEvent::TurnEnded(ended) => {
                record = ended;
                break;
            }
        }
    }
    assert_eq!(record.agent_session, canonical);
    assert!(
        matches!(record.stop, TurnStop::Failed { .. }),
        "the provider died mid-turn; the turn is failed, not completed or cancelled"
    );
    assert_eq!(
        turn.recv(),
        None,
        "a lane whose bridge died ends instead of blocking its reader"
    );
    assert_eq!(lane.recv(), None);
    assert!(
        host.transport_error().is_some(),
        "a transport death is recorded, so a caller can say why the lane ended"
    );
    // And the stopped bridge refuses new work honestly rather than parking it.
    assert_eq!(
        lane.prompt(json!([{ "type": "text", "text": "fast after death" }]))
            .unwrap_err()
            .code(),
        "agent_session_host.transport_closed"
    );
    drop(host);
}

#[test]
fn stopping_the_host_wakes_a_waiter_parked_on_an_in_flight_turn() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-stop-wake");
    let lane = open(&host, "agent-session/host-stop-wake");
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow stop wake" }]))
        .unwrap();
    let first = turn.recv().unwrap();
    assert!(
        chunk_text(&first).is_some(),
        "the turn is in flight when the host stops"
    );

    // The waiter parks on the turn; the host stopping is the only thing that
    // can end the wait. If the stop stranded waiters, this join never returned.
    let waiter = std::thread::spawn(move || turn.wait());
    host.shutdown().unwrap();
    let waited = waiter
        .join()
        .unwrap()
        .expect("a caller parked on an in-flight turn must learn of the stop");
    assert_eq!(waited.agent_session, canonical);
    assert_eq!(
        waited.stop,
        TurnStop::Failed {
            reason: "the host was stopped deliberately while the turn was in flight".into()
        }
    );
    // The lane ends too, so the next reader is not parked on a dead transport.
    assert_eq!(lane.recv(), None);
}

#[test]
fn an_interrupt_that_loses_the_race_records_no_false_interruption() {
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-race");
    let lane = open(&host, "agent-session/host-race");
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow raceignore" }]))
        .unwrap();
    let first = turn.recv().unwrap();
    assert!(
        chunk_text(&first).is_some(),
        "the cancel races a running turn"
    );

    // The cancel is issued and delivered, and the fixture finishes the turn on
    // its own anyway. The host must not dress a completed turn up as an
    // interrupted one.
    let receipt = turn.interrupt(Some("raced completion".into())).unwrap();
    assert_eq!(receipt.agent_session, canonical);
    assert_eq!(receipt.commands, vec!["session/cancel".to_string()]);

    let record = turn.wait().unwrap();
    assert_eq!(
        record.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    assert!(
        record.interruption.is_none(),
        "a turn that ran to completion is not an interrupted turn"
    );
    assert_eq!(
        host.interruptions(&canonical).unwrap(),
        Vec::new(),
        "the trail records no interruption for a turn nobody stopped"
    );
    assert_eq!(
        host.identity(&canonical).unwrap().state,
        SessionLaneState::Resident
    );
    host.shutdown().unwrap();
}

#[test]
fn dropping_a_host_without_shutdown_does_not_hang_the_caller() {
    let Some(host) = launch() else {
        return;
    };
    let lane = open(&host, "agent-session/host-drop");
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "slow drop" }]))
        .unwrap();
    let first = turn.recv().unwrap();
    assert!(chunk_text(&first).is_some());
    let waiter = std::thread::spawn(move || turn.wait());
    // Deliberate drop while the fixture is alive, the reader is blocked on its
    // stdout, and a caller is parked on the turn: the drop path must wake the
    // waiter and terminate the process, or neither this join nor the test
    // would ever return.
    drop(host);
    let waited = waiter
        .join()
        .unwrap()
        .expect("a caller parked on an in-flight turn must learn of the drop");
    assert!(matches!(waited.stop, TurnStop::Failed { .. }));
}

#[test]
fn the_stated_turn_bound_is_a_guard_and_not_the_ordinary_turn_end() {
    assert_eq!(
        AgentSessionHostLimits::default().max_signals_per_turn,
        DEFAULT_MAX_SIGNALS_PER_TURN
    );
    let Some(host) = launch() else {
        return;
    };
    let canonical = r("agent-session/host-limit");
    let lane = open(&host, "agent-session/host-limit");
    let turn = lane
        .prompt(json!([{ "type": "text", "text": "fast bounded turn" }]))
        .unwrap();
    let record = turn.wait().unwrap();
    assert_eq!(record.agent_session, canonical);
    assert!(record.signals < DEFAULT_MAX_SIGNALS_PER_TURN);
    assert_eq!(
        record.stop,
        TurnStop::Completed {
            stop_reason: "end_turn".into()
        }
    );
    host.shutdown().unwrap();
}
