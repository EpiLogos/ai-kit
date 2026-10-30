//! A decision helper's answer is kept against the agent session that asked (O-I #684, DLW manual §19.4 Ask row): `aikit
//! decide invoke` keeps each ask with the session named by `--agent-session`, or by the session's own `AIKIT_SESSION_ID`,
//! and `aikit decide asks --agent-session` reads them back: the labels each question allowed and the answer, as evidence.
//! A loopback controlled server stands in for the decision endpoint; nothing here contacts a hosted API, and every run
//! has its own HOME and AIKIT_HOME.
use aikit_core::{ResourceRef, SessionId};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
};

const MODEL: &str = "kev-latest";
const ASKER: &str = "agent-session/anima-cf5";

/// A loopback endpoint that answers `answers` POSTs, each with the body given, then stops.
fn endpoint(answers: Vec<Value>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let done = thread::spawn(move || {
        for body in answers {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            loop {
                let n = socket.read(&mut buffer).unwrap();
                bytes.extend_from_slice(&buffer[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .map(|n| n.trim().parse::<usize>().unwrap())
                        .unwrap_or(0);
                    while bytes.len() < end + 4 + length {
                        let n = socket.read(&mut buffer).unwrap();
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    break;
                }
            }
            let encoded = serde_json::to_vec(&body).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                encoded.len()
            );
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.write_all(&encoded);
        }
    });
    (address, done)
}

struct World {
    root: tempfile::TempDir,
}

impl World {
    fn new(address: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        std::fs::write(
            root.path().join("provider.json"),
            serde_json::to_vec_pretty(&json!({
                "schema": "aikit.decision-provider/v1",
                "mode": "managed-local",
                "address": address,
                "limits": {"timeout_ms": 5000, "max_attempts": 1, "max_input_tokens_per_attempt": 8000,
                           "max_output_tokens_per_attempt": 2000, "model": MODEL},
                "decision_model": {"family": "kev", "artifact": "jaredpalmer/kev-0.8b", "base": "Qwen/Qwen3.5-0.8B-Base",
                    "base_revision": "dc7cdfe2ee4154fa7e30f5b51ca41bfa40174e68", "runtime": "kev.serve (uv)",
                    "backend": "mlx", "precision": "bf16-as-stored", "calibration": "checkpoint-fitted-temperature",
                    "license": "Apache-2.0"}
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.path().join("request.json"),
            serde_json::to_vec(&json!({
                "model": MODEL,
                "state": {"passage": "The cradle root is untouched."},
                "questions": {"standing": {"type": "choice", "instructions": "Which standing fits the claim?",
                    "criteria": {"fact": "observed", "claim": "asserted", "inference": "derived", "guess": "unsupported"}}}
            }))
            .unwrap(),
        )
        .unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn aikit(&self, args: &[&str], session_env: Option<&str>) -> Value {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aikit"));
        command
            .arg("--json")
            .args(args)
            .current_dir(self.root.path())
            .env("HOME", self.path("home"))
            .env("AIKIT_HOME", self.path("state"))
            .env_remove("AIKIT_SESSION_ID")
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("CENTRAL_ROOT")
            .stdin(Stdio::null());
        if let Some(session) = session_env {
            command.env("AIKIT_SESSION_ID", session);
        }
        let output = command.output().unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "aikit {args:?}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn ask(&self, session: Option<&str>, session_env: Option<&str>) -> Value {
        let provider = self.path("provider.json");
        let request = self.path("request.json");
        let mut args = vec![
            "decide",
            "invoke",
            "--provider-file",
            provider.to_str().unwrap(),
            "--request-file",
            request.to_str().unwrap(),
        ];
        if let Some(session) = session {
            args.extend(["--agent-session", session]);
        }
        self.aikit(&args, session_env)
    }

    fn asks(&self, session: &str) -> Value {
        self.aikit(&["decide", "asks", "--agent-session", session], None)
    }
}

/// The helper's answer, its pick the label it holds most strongly.
fn answer(choice: &str) -> Value {
    let mut probabilities = json!({"fact": 0.1, "claim": 0.1, "inference": 0.1, "guess": 0.1});
    probabilities[choice] = json!(0.7);
    json!({
        "model": MODEL,
        "answers": {"standing": {"type": "choice", "choice": choice, "probabilities": probabilities, "confidence": 0.7}},
        "usage": {"input_tokens": 61, "output_tokens": 3}
    })
}

fn only_file(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| entries.map(|entry| entry.unwrap().path()).collect())
        .unwrap_or_default()
}

#[test]
fn an_answer_is_kept_against_the_session_that_asked_and_read_back_by_its_ref() {
    let (address, done) = endpoint(vec![answer("inference"), answer("claim")]);
    let world = World::new(&address);

    // Named on the command line.
    let first = world.ask(Some(ASKER), None);
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(first["data"]["outcome"], "completed");
    // Named by the session's own process identity, as AIKit launches it.
    let own = SessionId::of_agent_session(&ResourceRef::parse(ASKER).unwrap());
    let second = world.ask(None, Some(own.as_str()));
    assert_eq!(second["ok"], true, "{second}");
    done.join().unwrap();

    let read = world.asks(ASKER);
    assert_eq!(read["ok"], true, "{read}");
    let data = &read["data"];
    assert_eq!(data["schema"], "aikit.decision-asks/v1");
    assert_eq!(data["agent_session"], ASKER);
    assert_eq!(data["session_id"], own.as_str());
    let asks = data["asks"].as_array().unwrap();
    assert_eq!(asks.len(), 2, "{data}");
    let ask = &asks[0];
    assert_eq!(ask["schema"], "aikit.decision-ask/v1");
    assert_eq!(ask["agent_session"], ASKER);
    assert_eq!(ask["outcome"], "answered");
    assert_eq!(ask["model"], MODEL);
    assert_eq!(
        ask["questions"]["standing"]["instructions"],
        "Which standing fits the claim?"
    );
    let labels = ask["questions"]["standing"]["criteria"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(labels, ["claim", "fact", "guess", "inference"]);
    assert_eq!(ask["answers"]["standing"]["choice"], "inference");
    assert!(ask["invocation_ref"]
        .as_str()
        .is_some_and(|r| !r.is_empty()));
    assert!(ask["asked_at"].as_str().is_some_and(|at| at.ends_with('Z')));
    assert_eq!(
        asks[1]["answers"]["standing"]["choice"], "claim",
        "{}",
        asks[1]
    );

    // Another session's thread holds none of them.
    let other = world.asks("agent-session/aletheia-gate");
    assert_eq!(other["data"]["asks"], json!([]));
}

#[test]
fn an_unanswered_ask_is_kept_with_why_and_an_ask_with_no_session_is_kept_nowhere() {
    // Port 9 on loopback: nothing answers there.
    let world = World::new("127.0.0.1:9");
    let unanswered = world.ask(Some(ASKER), None);
    assert_eq!(unanswered["data"]["outcome"], "failed", "{unanswered}");
    let read = world.asks(ASKER);
    let ask = &read["data"]["asks"][0];
    assert_eq!(ask["outcome"], "unanswered");
    assert!(ask["failure"].as_str().is_some_and(|why| !why.is_empty()));
    assert!(ask.get("answers").is_none_or(Value::is_null));

    let nobody = World::new("127.0.0.1:9");
    nobody.ask(None, None);
    assert!(only_file(&nobody.path("state/sessions")).is_empty());
}
