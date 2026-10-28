//! A take's body with the session's own agent (O-I #795): the real `aikit
//! session-space encounter-take-body` binary speaking Actuation's
//! agent-jsonl protocol, a real resident encounter owner on a temporary home,
//! and AIKit's controlled ACP protocol peer standing in for the provider (not
//! a model): it advertises a model, an effort and permission modes, and acts
//! in the cwd its session was opened with by writing what it was set to.
#![cfg(unix)]
use aikit_cli::encounter_service::{
    request, serve, EncounterProvider, EncounterRequest, EncounterService,
};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::ResourceRef;
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

const PEER: &str = r#"
import json, os, sys, threading
log = sys.argv[1]
sessions = {}
waiting = {}
def emit(value):
    os.write(1, (json.dumps(value) + "\n").encode())
def result(ident, value):
    emit({"jsonrpc": "2.0", "id": ident, "result": value})
def config(s):
    return [{"id": "model", "category": "model", "type": "select", "name": "Model", "currentValue": s["model"],
             "options": [{"value": "test/a", "name": "A"}, {"value": "test/b", "name": "B"}]},
            {"id": "reasoning_effort", "category": "thought_level", "type": "select", "name": "Effort",
             "currentValue": s["effort"], "options": [{"value": v, "name": v} for v in ("low", "medium", "high")]}]
def modes(s):
    return {"currentModeId": s["mode"], "availableModes": [{"id": m, "name": m} for m in ("ask", "edit", "auto")]}
def note(entry):
    with open(log, "a") as out:
        out.write(json.dumps(entry) + "\n")
for line in sys.stdin:
    message = json.loads(line)
    method, ident, params = message.get("method"), message.get("id"), message.get("params", {})
    if method == "initialize":
        result(ident, {"protocolVersion": 1, "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"resume": True}}})
    elif method == "session/new":
        native = "take-native-%d" % (len(sessions) + 1)
        sessions[native] = {"cwd": params.get("cwd"), "model": "test/a", "effort": "medium", "mode": "ask"}
        note({"new": native, "cwd": params.get("cwd")})
        result(ident, {"sessionId": native, "configOptions": config(sessions[native]), "modes": modes(sessions[native])})
    elif method == "session/set_config_option":
        s = sessions[params["sessionId"]]
        s["model" if params["configId"] == "model" else "effort"] = params["value"]
        result(ident, {"configOptions": config(s)})
    elif method == "session/set_mode":
        sessions[params["sessionId"]]["mode"] = params["modeId"]
        result(ident, {})
    elif method == "session/prompt":
        native = params["sessionId"]
        s = sessions[native]
        prompt = "".join(item.get("text", "") for item in params["prompt"])
        note({"prompt": prompt, "session": native})
        if "ASK_PERMISSION" in prompt:
            waiting[native] = ident
            emit({"jsonrpc": "2.0", "id": "permission-1", "method": "session/request_permission", "params": {
                "sessionId": native, "toolCall": {"toolCallId": "t1", "title": "Write a file", "kind": "edit", "status": "pending"},
                "options": [{"optionId": "deny", "name": "Deny", "kind": "reject_once"}, {"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}})
            continue
        said = "model=%s effort=%s mode=%s" % (s["model"], s["effort"], s["mode"])
        with open(os.path.join(s["cwd"], "answer.txt"), "w") as out:
            out.write(said + "\n")
        emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": native,
              "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "wrote answer.txt: " + said}}}})
        result(ident, {"stopReason": "end_turn", "usage": {"totalTokens": 7, "inputTokens": 5, "outputTokens": 2}})
    elif method == "session/cancel":
        native = params.get("sessionId")
        if native in waiting:
            result(waiting.pop(native), {"stopReason": "cancelled"})
    elif ident == "permission-1":
        for native in list(waiting):
            result(waiting.pop(native), {"stopReason": "cancelled"})
"#;

struct Rig {
    dir: tempfile::TempDir,
    home: AikitHome,
    socket: PathBuf,
    log: PathBuf,
    pid: u32,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Rig {
    /// Stops the owner this test started, by the pid it answered.
    fn drop(&mut self) {
        let _ = request(
            &self.socket,
            &EncounterRequest::Shutdown {
                expected_pid: self.pid,
            },
        );
        if let Some(server) = self.server.take() {
            let _ = server.join();
        }
    }
}

impl Rig {
    fn new() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("aikit"));
        home.ensure_layout().unwrap();
        let script = root.join("peer.py");
        std::fs::write(&script, PEER).unwrap();
        let log = root.join("peer.log");
        EncounterService::configure(
            &home,
            EncounterProvider {
                protocol: Default::default(),
                id: "take-peer".into(),
                label: "Controlled protocol peer (not a model)".into(),
                argv: vec![
                    "python3".into(),
                    "-u".into(),
                    script.display().to_string(),
                    log.display().to_string(),
                ],
                body_ref: None,
                body_revision: None,
                from_profile: None,
                argv_fallback: Vec::new(),
                env: Default::default(),
                cwd: None,
                required_context: None,
                model_policy: None,
                now_context: None,
            },
        )
        .unwrap();
        let socket = root.join("ipc/owner.sock");
        let serving = (home.clone(), socket.clone());
        let server = std::thread::spawn(move || serve(serving.0, &serving.1).unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        let pid = loop {
            if let Ok(health) = request(&socket, &EncounterRequest::Health) {
                if health["ok"] == true {
                    break health["data"]["pid"].as_u64().unwrap() as u32;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the owner did not bind its socket"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        Rig {
            dir,
            home,
            socket,
            log,
            pid,
            server: Some(server),
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap()
    }

    fn owner(&self, request_: &EncounterRequest) -> Value {
        let answer = request(&self.socket, request_).unwrap();
        assert_eq!(answer["ok"], true, "{answer}");
        answer["data"].clone()
    }

    /// The session the takes are of, opened on the peer as any session is.
    fn own_session(&self, name: &str, open: bool) -> ResourceRef {
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        let space = SessionSpaceRef::parse(&format!("session-space/{name}")).unwrap();
        let session = ResourceRef::parse(format!("agent-session/{name}")).unwrap();
        store
            .apply(
                &store
                    .stage(
                        None,
                        SessionSpaceMutation::Create {
                            id: space.clone(),
                            label: None,
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        store
            .apply(
                &store
                    .stage(
                        Some(&space),
                        SessionSpaceMutation::AttachAgentSession {
                            attachment: SessionSpaceAgentAttachmentIntent {
                                agent_session: session.clone(),
                                purpose: Some("the session the takes are of".into()),
                                provenance: vec!["take body acceptance".into()],
                            },
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
        if open {
            let cwd = self.root().join(format!("{name}-cwd"));
            std::fs::create_dir_all(&cwd).unwrap();
            self.owner(&EncounterRequest::Open {
                space,
                agent_session: session.clone(),
                provider: "take-peer".into(),
                cwd,
            });
        }
        session
    }

    fn world(&self, name: &str) -> PathBuf {
        let world = self.root().join("takes").join(name);
        std::fs::create_dir_all(&world).unwrap();
        world
    }

    fn body(&self, session: &ResourceRef, socket: &Path) -> Body {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .args(["session-space", "encounter-take-body", "--session"])
            .arg(session.as_str())
            .arg("--socket")
            .arg(socket)
            .args(["--turn-timeout-secs", "20"])
            // Actuation clears a body's environment: only what the run names reaches it.
            .env_clear()
            .env("AIKIT_HOME", self.home.root())
            .env("HOME", self.home.root())
            .current_dir(self.root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Body {
            child,
            input: Some(input),
            output,
            n: 0,
        }
    }

    fn peer_log(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

struct Body {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    n: u64,
}

impl Body {
    /// One protocol exchange, as Actuation's RPC client makes it.
    fn ask(&mut self, mut command: Value) -> Value {
        self.n += 1;
        let id = format!("actuation-prime-{}", self.n);
        command["id"] = json!(id);
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{command}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        assert!(
            !line.is_empty(),
            "the body closed its output: {}",
            self.stderr()
        );
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(reply["type"], "response", "{reply}");
        assert_eq!(reply["id"], json!(id), "{reply}");
        assert_eq!(reply["command"], command["type"], "{reply}");
        reply
    }

    fn stderr(&mut self) -> String {
        drop(self.input.take());
        let _ = self.child.wait();
        let mut text = String::new();
        if let Some(mut err) = self.child.stderr.take() {
            use std::io::Read;
            let _ = err.read_to_string(&mut text);
        }
        text
    }

    fn exits(mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "the body exited {status}");
                return;
            }
            assert!(Instant::now() < deadline, "the body did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn preflight(session: &ResourceRef, world: &Path, trace: &str, model: &str, level: Value) -> Value {
    let mut configuration = json!({"provider": "take-peer", "model": model, "agent": session.as_str(),
        "trace_ref": trace, "world": world});
    if !level.is_null() {
        configuration["level"] = level;
    }
    json!({"type": "preflight", "configuration": configuration})
}

#[test]
fn a_take_opens_a_fresh_resident_of_the_sessions_agent_applies_its_level_and_acts_in_its_world() {
    let rig = Rig::new();
    let own = rig.own_session("own", true);
    let world = rig.world("trial-000-high");
    let mut body = rig.body(&own, &rig.socket);

    let level = json!({"variable": "effort", "value": "high"});
    let ready = body.ask(preflight(
        &own,
        &world,
        "takes:own:1:trial:0",
        "test/b",
        level.clone(),
    ));
    assert_eq!(ready["success"], true, "{ready}");
    let data = &ready["data"];
    assert_eq!(data["ready"], true);
    assert_eq!(data["provider"], "take-peer");
    assert_eq!(data["model"], "test/b", "the configuration's model is held");
    assert_eq!(data["agent"], own.as_str());
    assert_eq!(data["level"], level, "the level is read back as given");
    assert_eq!(data["applied"]["reasoning_effort"], "high");
    let take = data["agent_session"].as_str().unwrap().to_owned();
    assert_ne!(
        take,
        own.as_str(),
        "a fresh resident, never the session's own"
    );
    let opened_in: Vec<Value> = rig
        .peer_log()
        .into_iter()
        .filter(|row| row.get("new").is_some())
        .collect();
    assert_eq!(opened_in.len(), 2, "the session's own, then the take's");
    assert_eq!(
        opened_in[1]["cwd"],
        json!(world),
        "the take's agent stands in its World"
    );

    let turned = body.ask(json!({"type": "turn", "turn": {"iteration": 1, "prompt": "Make answer.txt say what you are set to."}}));
    assert_eq!(turned["success"], true, "{turned}");
    assert_eq!(turned["data"]["status"], "completed", "{turned}");
    assert_eq!(
        turned["data"]["output"],
        "wrote answer.txt: model=test/b effort=high mode=ask"
    );
    assert_eq!(
        turned["data"]["usage"]["total_tokens"], 7,
        "the provider's own usage"
    );
    assert_eq!(
        std::fs::read_to_string(world.join("answer.txt")).unwrap(),
        "model=test/b effort=high mode=ask\n",
        "the agent acted in its World"
    );

    let completed = body.ask(json!({"type": "complete", "request": {"system": "Answer as asked.", "prompt": "Say it again.", "purpose": "ql-act", "temperature": 0}}));
    assert_eq!(completed["success"], true, "{completed}");
    assert_eq!(completed["data"]["usage"]["total_tokens"], 7);
    let prompts: Vec<String> = rig
        .peer_log()
        .into_iter()
        .filter_map(|row| row["prompt"].as_str().map(str::to_owned))
        .collect();
    assert!(
        prompts
            .last()
            .unwrap()
            .contains("Answer as asked.\n\nSay it again."),
        "{prompts:?}"
    );

    let finished = body.ask(json!({"type": "finalize", "inspection": {}}));
    assert_eq!(finished["success"], true, "{finished}");
    assert_eq!(finished["data"]["released"], true, "{finished}");
    assert_eq!(finished["data"]["turns"], 2);
    body.exits();

    let status = rig.owner(&EncounterRequest::Status {
        agent_session: ResourceRef::parse(take).unwrap(),
    });
    assert_eq!(status["state"], "Released", "{status}");
    // The session's own resident was never touched: its model stands where it was.
    let own_model = rig.owner(&EncounterRequest::ModelRead {
        agent_session: own.clone(),
    });
    assert_eq!(own_model["model_observation"]["current_model_id"], "test/a");
    assert_eq!(
        own_model["model_observation"]["reasoning_effort"]["currentValue"],
        "medium"
    );
}

#[test]
fn a_take_places_its_autonomy_on_the_agents_modes_and_a_permission_ask_is_cancelled() {
    let rig = Rig::new();
    let own = rig.own_session("own", true);
    let world = rig.world("trial-000-auto");
    let mut body = rig.body(&own, &rig.socket);
    let level = json!({"variable": "autonomy", "value": "auto"});
    let ready = body.ask(preflight(
        &own,
        &world,
        "takes:own:2:trial:0",
        "test/a",
        level.clone(),
    ));
    assert_eq!(ready["success"], true, "{ready}");
    assert_eq!(ready["data"]["level"], level);
    assert_eq!(
        ready["data"]["applied"]["mode"], "auto",
        "the last word on the last mode"
    );
    let turned = body.ask(json!({"type": "turn", "turn": {"iteration": 1, "prompt": "Go."}}));
    assert_eq!(
        std::fs::read_to_string(world.join("answer.txt")).unwrap(),
        "model=test/a effort=medium mode=auto\n",
        "{turned}"
    );
    // A tool's permission is the person's to give: a take asks none of it.
    let asked =
        body.ask(json!({"type": "turn", "turn": {"iteration": 2, "prompt": "ASK_PERMISSION"}}));
    assert_eq!(asked["success"], true, "{asked}");
    assert_eq!(asked["data"]["status"], "failed", "{asked}");
    assert!(
        asked["data"]["reason"]
            .as_str()
            .unwrap()
            .contains("a take takes none"),
        "{asked}"
    );
    let finished = body.ask(json!({"type": "finalize", "inspection": {}}));
    assert_eq!(finished["data"]["released"], true, "{finished}");
    body.exits();
}

#[test]
fn a_take_body_refuses_in_words_what_is_not_its_sessions_agent_to_run() {
    let rig = Rig::new();
    let own = rig.own_session("own", true);
    let world = rig.world("refusals");
    let refused = |body: &mut Body, command: Value, words: &str| {
        let reply = body.ask(command);
        assert_eq!(reply["success"], false, "{reply}");
        let said = reply["data"]["error"].as_str().unwrap().to_owned();
        assert!(said.contains(words), "{said}");
    };
    let mut body = rig.body(&own, &rig.socket);
    let mut other = preflight(&own, &world, "takes:own:3:trial:0", "test/a", Value::Null);
    other["configuration"]["provider"] = json!("someone-else");
    refused(
        &mut body,
        other,
        "opens on the encounter provider take-peer",
    );
    body.exits();

    let mut body = rig.body(&own, &rig.socket);
    let mut stranger = preflight(&own, &world, "takes:own:3:trial:0", "test/a", Value::Null);
    stranger["configuration"]["agent"] = json!("agent-session/stranger");
    refused(
        &mut body,
        stranger,
        "the take's configuration names agent-session/stranger",
    );
    body.exits();

    let mut body = rig.body(&own, &rig.socket);
    let skillset = json!({"variable": "skillset", "value": "careful"});
    refused(
        &mut body,
        preflight(&own, &world, "takes:own:3:trial:0", "test/a", skillset),
        "a skillset has no owner operation on a fresh resident yet",
    );
    body.exits();

    let never = rig.own_session("never", false);
    let mut body = rig.body(&never, &rig.socket);
    refused(
        &mut body,
        preflight(
            &never,
            &world,
            "takes:never:1:trial:0",
            "test/a",
            Value::Null,
        ),
        "has never opened",
    );
    body.exits();

    let gone = rig.root().join("ipc/nobody.sock");
    let mut body = rig.body(&own, &gone);
    refused(
        &mut body,
        preflight(&own, &world, "takes:own:4:trial:0", "test/a", Value::Null),
        "The resident encounter owner does not answer",
    );
    body.exits();
    // Nothing opened for any refusal: only the session's own agent ever did.
    let opened = rig
        .peer_log()
        .into_iter()
        .filter(|row| row.get("new").is_some())
        .count();
    assert_eq!(opened, 1);
}
