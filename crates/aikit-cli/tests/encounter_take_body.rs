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
        # The skills the harness finds in its project tree at session start (pi reads `.pi/skills`).
        found = {}
        root = os.path.join(params.get("cwd") or "", ".pi", "skills")
        if os.path.isdir(root):
            for name in sorted(os.listdir(root)):
                rules = os.path.join(root, name, "references", "rules.md")
                if os.path.isfile(rules):
                    found[name] = open(rules).read().strip()
        note({"new": native, "cwd": params.get("cwd"), "skills": found})
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
                "options": [{"optionId": "allow_once", "name": "Allow once", "kind": "allow_once"},
                            {"optionId": "allow_session", "name": "Allow for session", "kind": "allow_always"},
                            {"optionId": "allow_always", "name": "Allow always", "kind": "allow_always"},
                            {"optionId": "deny", "name": "Deny", "kind": "reject_once"}]}})
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
        outcome = (message.get("result") or {}).get("outcome") or {}
        chosen = outcome.get("optionId") if outcome.get("outcome") == "selected" else None
        note({"permission": chosen})
        for native in list(waiting):
            if chosen and chosen.startswith("allow"):
                with open(os.path.join(sessions[native]["cwd"], "answer.txt"), "w") as out:
                    out.write("permission=%s\n" % chosen)
                result(waiting.pop(native), {"stopReason": "end_turn"})
            else:
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
        Rig::launched_as(None)
    }

    /// The peer launched as the harness named `program` (a wrapper of that
    /// name), so AIKit reads the provider's harness from its program as it
    /// does for a real one: `pi` reads its skills from `.pi/skills`.
    fn launched_as(program: Option<&str>) -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = AikitHome::at(root.join("aikit"));
        home.ensure_layout().unwrap();
        let script = root.join("peer.py");
        std::fs::write(&script, PEER).unwrap();
        let log = root.join("peer.log");
        let argv = match program {
            None => vec![
                "python3".into(),
                "-u".into(),
                script.display().to_string(),
                log.display().to_string(),
            ],
            Some(program) => {
                let python = String::from_utf8(
                    Command::new("sh")
                        .args(["-c", "command -v python3"])
                        .output()
                        .unwrap()
                        .stdout,
                )
                .unwrap();
                let wrapper = root.join("bin").join(program);
                std::fs::create_dir_all(wrapper.parent().unwrap()).unwrap();
                std::fs::write(
                    &wrapper,
                    format!(
                        "#!/bin/sh\nexec {} -u {} \"$@\"\n",
                        python.trim(),
                        script.display()
                    ),
                )
                .unwrap();
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
                vec![wrapper.display().to_string(), log.display().to_string()]
            }
        };
        EncounterService::configure(
            &home,
            EncounterProvider {
                protocol: Default::default(),
                id: "take-peer".into(),
                label: "Controlled protocol peer (not a model)".into(),
                argv,
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
    // O-I manual §19.1: each take keeps its own NOW, a child of its task's. This session
    // holds no prepared task, so the take keeps none, and says so.
    assert!(
        data["now"]["none"]
            .as_str()
            .is_some_and(|words| words.contains("holds no prepared task")),
        "{data}"
    );
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
fn a_takes_tool_calls_answer_the_harnesss_request_in_its_own_words_and_ask_waits() {
    let rig = Rig::new();
    let own = rig.own_session("own", true);
    // Session: the harness's own answer for its session, read back from what the peer was given.
    let world = rig.world("trial-000-session");
    let mut body = rig.body(&own, &rig.socket);
    let level = json!({"variable": "tool_calls", "value": "session"});
    let ready = body.ask(preflight(
        &own,
        &world,
        "takes:own:2:trial:0",
        "test/a",
        level.clone(),
    ));
    assert_eq!(ready["success"], true, "{ready}");
    assert_eq!(ready["data"]["level"], level);
    assert_eq!(ready["data"]["applied"]["tool_calls"], "session");
    let asked =
        body.ask(json!({"type": "turn", "turn": {"iteration": 1, "prompt": "ASK_PERMISSION"}}));
    assert_eq!(asked["data"]["status"], "completed", "{asked}");
    assert_eq!(
        std::fs::read_to_string(world.join("answer.txt")).unwrap(),
        "permission=allow_session\n",
        "{asked}"
    );
    assert!(
        rig.peer_log()
            .iter()
            .any(|entry| entry["permission"] == "allow_session"),
        "the harness was answered with its own session option"
    );
    let finished = body.ask(json!({"type": "finalize", "inspection": {}}));
    assert_eq!(finished["data"]["released"], true, "{finished}");
    body.exits();

    // Ask: the request waits for a person, and nobody is asked in a take.
    let world = rig.world("trial-001-ask");
    let mut body = rig.body(&own, &rig.socket);
    let level = json!({"variable": "tool_calls", "value": "ask"});
    let ready = body.ask(preflight(
        &own,
        &world,
        "takes:own:2:trial:1",
        "test/a",
        level.clone(),
    ));
    assert_eq!(ready["data"]["level"], level, "{ready}");
    let asked =
        body.ask(json!({"type": "turn", "turn": {"iteration": 1, "prompt": "ASK_PERMISSION"}}));
    assert_eq!(asked["success"], true, "{asked}");
    assert_eq!(asked["data"]["status"], "failed", "{asked}");
    assert!(
        asked["data"]["reason"]
            .as_str()
            .unwrap()
            .contains("Tool calls stands at ask"),
        "{asked}"
    );
    assert!(!world.join("answer.txt").exists());
    let finished = body.ask(json!({"type": "finalize", "inspection": {}}));
    assert_eq!(finished["data"]["released"], true, "{finished}");
    body.exits();

    // Autonomy is retired: a take that varies it is refused, naming what took its place.
    let mut body = rig.body(&own, &rig.socket);
    let refused = body.ask(preflight(
        &own,
        &world,
        "takes:own:3:trial:0",
        "test/a",
        json!({"variable": "autonomy", "value": "auto"}),
    ));
    assert_eq!(refused["success"], false, "{refused}");
    assert!(
        refused["data"]["error"]
            .as_str()
            .unwrap()
            .contains("Scope's Tool calls"),
        "{refused}"
    );
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

    // A set is carried into the World where its harness reads skills; the peer's
    // harness (a bare python3) is none AIKit knows, so it carries no set.
    let mut body = rig.body(&own, &rig.socket);
    let skillset = json!({"variable": "skillset", "value": "careful"});
    refused(
        &mut body,
        preflight(&own, &world, "takes:own:3:trial:0", "test/a", skillset),
        "names no harness whose skill tree AIKit knows",
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

/// A Skillset level (O-I manual §19.1: "a level is a set of skills, or that set
/// with one proposed revision of a skill in it. The revision is in effect for
/// that take only … each take's return shows which revision it worked under"):
/// the body carries the set into the take's World where the harness reads
/// skills, the proposed revision in place of the member's revision in use, and
/// answers what it read back from the World; the proposal keeps the take.
#[test]
fn a_skillset_take_carries_its_set_into_its_world_with_the_proposed_revision_for_that_take_only() {
    let rig = Rig::launched_as(Some("pi"));
    let own = rig.own_session("own", true);
    let skill = |dir: &Path, name: &str, marker: &str| {
        std::fs::create_dir_all(dir.join("references")).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: A member of the set a take carries.\n---\n\n{marker}\n"),
        )
        .unwrap();
        std::fs::write(dir.join("references/rules.md"), format!("{marker}\n")).unwrap();
    };
    let source = rig.root().join("practice");
    skill(&source.join("alpha"), "alpha", "alpha-one");
    skill(&source.join("beta"), "beta", "beta-one");
    aikit_cli::skill_sources::add_directory(&rig.home, "practice", &source, false).unwrap();
    aikit_cli::skill_sources::sync(&rig.home, "practice").unwrap();
    aikit_cli::skill_sources::promote(&rig.home, "practice", false, &[]).unwrap();
    let alpha = aikit_core::CapsuleId::parse("skill/practice/alpha").unwrap();
    let beta = aikit_core::CapsuleId::parse("skill/practice/beta").unwrap();
    aikit_store::skillsets::create(&rig.home, "docs", &[alpha, beta], &[]).unwrap();
    let draft = rig.root().join("drafts/alpha");
    skill(&draft, "alpha", "alpha-two");
    let proposal = aikit_cli::skill_sources::propose_revision(
        &rig.home,
        "skill/practice/alpha",
        &draft,
        vec![aikit_cli::skill_sources::RefinementReason {
            task_ref: "task:plan".into(),
            reason: "Not met: the plan cites the repository's own gates".into(),
        }],
    )
    .unwrap();
    let family = json!(["takes:own:5:trial:0", "takes:own:5:trial:1"]);
    // What Actuation read each take to have answered to the task's Success
    // conditions, handed to the body when it finalizes (research execution.rs).
    let answered = |trace: &str| {
        if trace.ends_with(":0") {
            json!([{"condition": "The plan cites the repository's own gates", "answer": "met",
                "evidence": [{"file": "plan.md", "digest": "d-plan"}], "why": "plan.md names them"}])
        } else {
            json!([{"condition": "The plan cites the repository's own gates", "answer": "not met",
                "evidence": [{"test": {"answer": "not met", "code": 1}}],
                "why": "Held to the task's own test, which answered not met"}])
        }
    };
    let take = |trace: &str, value: &str| {
        let world = rig.world(trace.rsplit(':').next().unwrap());
        let mut body = rig.body(&own, &rig.socket);
        let mut command = preflight(
            &own,
            &world,
            trace,
            "test/a",
            json!({"variable": "skillset", "value": value}),
        );
        command["configuration"]["takes"] = family.clone();
        let reply = body.ask(command);
        assert_eq!(reply["success"], true, "{reply}");
        let finalize = json!({"type": "finalize", "inspection": {"answers": answered(trace)}});
        assert_eq!(body.ask(finalize)["success"], true);
        body.exits();
        (world, reply["data"]["applied"].clone())
    };

    let (revised_world, revised) = take("takes:own:5:trial:0", "docs+skill/practice/alpha");
    assert_eq!(revised["set"], "docs");
    assert_eq!(
        revised["root"], ".pi/skills",
        "where pi reads its project's skills"
    );
    assert_eq!(revised["proposed"]["member"], "skill/practice/alpha");
    assert_eq!(
        revised["proposed"]["revision"],
        json!(proposal.proposed.revision)
    );
    let members = revised["members"].as_array().unwrap();
    assert_eq!(members.len(), 2, "{revised}");
    let row = |id: &str| members.iter().find(|row| row["id"] == id).unwrap().clone();
    assert_eq!(
        row("skill/practice/alpha")["revision"],
        json!(proposal.proposed.revision)
    );
    assert_eq!(row("skill/practice/alpha")["proposed"], true);
    assert_eq!(row("skill/practice/beta")["proposed"], false);
    assert_eq!(
        std::fs::read_to_string(revised_world.join(".pi/skills/alpha/references/rules.md"))
            .unwrap(),
        "alpha-two\n",
        "the proposed revision stands in this take's World"
    );
    let (in_use_world, in_use) = take("takes:own:5:trial:1", "docs");
    assert_eq!(in_use["proposed"], Value::Null);
    assert_eq!(
        std::fs::read_to_string(in_use_world.join(".pi/skills/alpha/references/rules.md")).unwrap(),
        "alpha-one\n",
        "the take beside it carries the revision in use"
    );
    // The harness found them there at session start.
    let opened: Vec<Value> = rig
        .peer_log()
        .into_iter()
        .filter(|row| row.get("new").is_some())
        .collect();
    assert_eq!(opened[1]["skills"]["alpha"], "alpha-two");
    assert_eq!(opened[2]["skills"]["alpha"], "alpha-one");
    // Nothing in use changed, and the proposal keeps the take that ran under it, beside its family.
    let listed = aikit_cli::skill_sources::refinements(&rig.home).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].tests.len(), 1, "{:?}", listed[0].tests);
    assert_eq!(listed[0].tests[0].trace, "takes:own:5:trial:0");
    assert_eq!(
        listed[0].tests[0].beside,
        vec!["takes:own:5:trial:1".to_owned()]
    );
    assert_eq!(listed[0].in_use.revision, proposal.in_use.revision);
    // Each take's answers to the task's Success conditions are kept with the
    // Test, the take under the proposed revision's and the take beside it's
    // (§19.7 Result: "What each take answered to the task's Success conditions").
    let answers = &listed[0].tests[0].answers;
    assert_eq!(answers.len(), 2, "{answers:?}");
    let revised_answer = &answers["takes:own:5:trial:0"][0];
    assert_eq!(
        revised_answer.condition,
        "The plan cites the repository's own gates"
    );
    assert_eq!(revised_answer.answer, "met");
    assert_eq!(
        revised_answer.evidence,
        vec!["file plan.md (d-plan)".to_owned()]
    );
    assert_eq!(revised_answer.why.as_deref(), Some("plan.md names them"));
    let in_use_answer = &answers["takes:own:5:trial:1"][0];
    assert_eq!(in_use_answer.answer, "not met");
    assert_eq!(
        in_use_answer.evidence,
        vec!["the task's own test: not met (exit 1)".to_owned()]
    );

    // A level names a set AIKit holds, and a member with a proposal standing.
    let refused = |value: &str, words: &str| {
        let world = rig.world("refused");
        let mut body = rig.body(&own, &rig.socket);
        let reply = body.ask(preflight(
            &own,
            &world,
            "takes:own:6:trial:0",
            "test/a",
            json!({"variable": "skillset", "value": value}),
        ));
        assert_eq!(reply["success"], false, "{reply}");
        let said = reply["data"]["error"].as_str().unwrap();
        assert!(said.contains(words), "{said}");
        body.exits();
    };
    refused("nowhere", "AIKit holds no set nowhere");
    refused(
        "docs+skill/practice/beta",
        "no proposed revision of skill/practice/beta stands",
    );
    refused(
        "docs+skill/other/gamma",
        "skill/other/gamma is not a member of docs",
    );
}
