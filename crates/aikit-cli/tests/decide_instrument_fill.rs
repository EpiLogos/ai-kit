//! Fill heads (O-I #843; DLW manual §19.1 Vāk and Generate): `aikit decide invoke --instrument vak-block` reads the instrument
//! from QL's team space (`ql context-frame team-space`, `ql.vak-team-space/v2`), asks the elected decision model one typed
//! question for each head the cell leaves open, among the labels that head holds, and returns the picks as evidence — never a
//! receipt, never admitted: the engine admits or refuses the block. A cell with no open head makes no call; a chosen label the
//! head does not hold, an instrument the team space does not hold and an instrument only the person answers are refused in
//! AIKit's words. A loopback server stands in for the decision endpoint and a script for `ql`; nothing here contacts a hosted
//! API, and every run has its own HOME and AIKIT_HOME.
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

const MODEL: &str = "kev-latest";
const ASKER: &str = "agent-session/factory-clip";

/// A loopback endpoint that answers `answers` POSTs, each with the body given, and keeps the bodies it was sent.
fn endpoint(answers: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    let done = thread::spawn(move || {
        for body in answers {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            let sent = loop {
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
                    break serde_json::from_slice::<Value>(&bytes[end + 4..end + 4 + length])
                        .unwrap_or(Value::Null);
                }
            };
            kept.lock().unwrap().push(sent);
            let encoded = serde_json::to_vec(&body).unwrap();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                encoded.len()
            );
            let _ = socket.write_all(head.as_bytes());
            let _ = socket.write_all(&encoded);
        }
    });
    (address, seen, done)
}

fn label(id: &str, description: &str, owner: &str, revision: &str) -> Value {
    json!({"id": id, "description": description, "owner_ref": owner, "owner_revision": revision})
}

fn head(id: &str, field: &str, policy: &str, max: u32, labels: Vec<Value>) -> Value {
    json!({"id": id, "field": field, "ambiguity_policy": policy, "cardinality": {"min": 0, "max": max}, "labels": labels})
}

/// QL's team space as `ql context-frame team-space --json` answers it, the vak-block instrument and one gate whole.
fn team_space() -> Value {
    let profile = "ql.vak-composition.profile/v1";
    let p = |field: &str| format!("ql:mef:vak-profile:{field}");
    let labels = |field: &str, ids: &[&str]| {
        ids.iter()
            .map(|id| label(id, &format!("{id} ({field})"), &p(field), profile))
            .collect::<Vec<_>>()
    };
    json!({
        "contract": "ql.vak-team-space/v2",
        "instruments": [
            {"id": "vak-block", "organ": "S4′ Anima", "kind": "vak-block", "source": "ql:skill:vak-evaluate",
             "answerable_by": "model-or-person", "seats": ["agent/anima"], "cells": ["CF5"],
             "heads": [
                head("vak-cpf", "participation", "abstain", 1, labels("participation", &["dialogical", "authorised-undertaking"])),
                head("vak-ct", "content-type", "preserve-candidates", 7, labels("content-type", &["CT0", "CT1", "CT2", "CT3", "CT4", "CT4b'", "CT5"])),
                head("vak-cp", "position", "abstain", 1, labels("position", &["4.0", "4.1", "4.2", "4.3", "4.4", "4.5"])),
                head("vak-cf", "context-frame", "abstain", 1, labels("context-frame", &["CF1", "CF2", "CF3", "CF4", "CF5", "CF6", "CF7"])),
                head("vak-cfp", "thread-form", "abstain", 1, labels("thread-form", &["CFP0", "CFP1", "CFP2", "CFP3", "CFP4", "CFP5"])),
                head("vak-cs", "sequence", "abstain", 1, labels("sequence", &["CS0", "CS1", "CS2", "CS3", "CS4", "CS5"])),
                head("vak-direction", "direction", "preserve-candidates", 2, vec![
                    label("forward-synthesis", "Prospective: the pass forward through the pairs", "ql:mef:cprime-oikonomia:direction", "ql.cprime-oikonomia/v1"),
                    label("returning-inquiry", "Retrospective: the pass back through the pairs", "ql:mef:cprime-oikonomia:direction", "ql.cprime-oikonomia/v1")]),
             ],
             "constraints": [{"id": "vak-z-needs-authorised-undertaking", "kind": "implication"}]},
            {"id": "gate-6", "organ": "S5′ Aletheia", "kind": "gate", "answerable_by": "person", "heads": [
                head("gate-6-recognition", "recognition", "abstain", 1, vec![label("recognised", "recognised", "ql:gate", "v1"), label("not-yet", "not yet", "ql:gate", "v1")])]}
        ],
        "rules": [
            {"id": "zero-heads-zero-calls", "text": "A cell with no unresolved head makes no model call."},
            {"id": "answer-is-evidence", "text": "A model answer is evidence for the engine to validate."},
            {"id": "engine-validates", "text": "The engine decides what holds: ql vak compose accepts or refuses the block."}
        ]
    })
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
                           "max_output_tokens_per_attempt": 2000, "model": MODEL}
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.path().join("team-space.json"),
            serde_json::to_vec(&team_space()).unwrap(),
        )
        .unwrap();
        let ql = root.path().join("ql");
        std::fs::write(
            &ql,
            format!(
                "#!/bin/sh\nif [ \"$1\" = context-frame ] && [ \"$2\" = team-space ] && [ \"$3\" = --json ]; then cat '{}'; else echo \"ql: unexpected $*\" >&2; exit 2; fi\n",
                root.path().join("team-space.json").display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ql, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn aikit(&self, args: &[&str]) -> Value {
        let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
            .arg("--json")
            .args(args)
            .current_dir(self.root.path())
            .env("HOME", self.path("home"))
            .env("AIKIT_HOME", self.path("state"))
            .env_remove("AIKIT_SESSION_ID")
            .env_remove("AIKIT_CONTEXT_ID")
            .env_remove("CENTRAL_ROOT")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "aikit {args:?}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn fill(&self, instrument: &str, chosen: Value) -> Value {
        std::fs::write(
            self.path("fill.json"),
            serde_json::to_vec(&json!({
                "schema": "aikit.instrument-fill-request/v1",
                "subject": {"ref": "workflow-unit:eros-verify", "key": "eros-verify",
                    "concern": "Verify the implementation against the plan with the repository's real gates",
                    "returns": "Return the commands run, their results and every failure found"},
                "chosen": chosen
            }))
            .unwrap(),
        )
        .unwrap();
        let (provider, request, ql) = (
            self.path("provider.json"),
            self.path("fill.json"),
            self.path("ql"),
        );
        self.aikit(&[
            "decide",
            "invoke",
            "--instrument",
            instrument,
            "--provider-file",
            provider.to_str().unwrap(),
            "--request-file",
            request.to_str().unwrap(),
            "--ql",
            ql.to_str().unwrap(),
            "--agent-session",
            ASKER,
        ])
    }
}

fn choice(choice: &str, labels: &[&str]) -> Value {
    let rest = (1.0 - 0.7) / (labels.len() - 1) as f64;
    let probabilities = labels
        .iter()
        .map(|l| (l.to_string(), json!(if *l == choice { 0.7 } else { rest })))
        .collect::<serde_json::Map<_, _>>();
    json!({"type": "choice", "choice": choice, "probabilities": probabilities, "confidence": 0.7})
}

#[test]
fn the_open_heads_are_asked_of_the_decision_model_and_come_back_as_evidence() {
    let answer = json!({"model": MODEL, "answers": {
        "vak-cs": choice("CS2", &["CS0", "CS1", "CS2", "CS3", "CS4", "CS5"]),
        "vak-direction": choice("returning-inquiry", &["forward-synthesis", "returning-inquiry"])},
        "usage": {"input_tokens": 220, "output_tokens": 6}});
    let (address, seen, done) = endpoint(vec![answer]);
    let world = World::new(&address);
    let filled = world.fill("vak-block", json!({"vak-cpf": ["dialogical"], "vak-ct": ["CT2"], "vak-cp": ["4.2"], "vak-cf": ["CF3"], "vak-cfp": ["CFP2"]}));
    done.join().unwrap();
    assert_eq!(filled["ok"], true, "{filled}");
    let data = &filled["data"];
    assert_eq!(data["schema"], "aikit.instrument-fill/v1");
    assert_eq!(data["instrument"]["id"], "vak-block");
    assert_eq!(data["team_space"], "ql.vak-team-space/v2");
    assert_eq!(data["outcome"], "completed");
    assert_eq!(data["open"], json!(["vak-cs", "vak-direction"]));
    assert_eq!(data["answered_by"], MODEL);
    let cs = &data["picks"]["vak-cs"];
    assert_eq!(
        (cs["field"].as_str(), cs["label"].as_str()),
        (Some("sequence"), Some("CS2"))
    );
    assert_eq!(cs["owner_revision"], "ql.vak-composition.profile/v1");
    assert_eq!(cs["confidence"], 0.7);
    assert_eq!(data["picks"]["vak-direction"]["label"], "returning-inquiry");
    assert_eq!(
        data["picks"]["vak-direction"]["owner_revision"],
        "ql.cprime-oikonomia/v1"
    );
    assert_eq!(data["standing"], "evidence");
    assert!(
        data["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "engine-validates"),
        "{data}"
    );
    // The model was asked only the open heads, each among the labels its head holds, over the cell's words and chosen heads.
    let sent = seen.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let questions = sent[0]["questions"].as_object().unwrap();
    assert_eq!(
        questions.keys().cloned().collect::<Vec<_>>(),
        ["vak-cs", "vak-direction"]
    );
    let criteria = questions["vak-direction"]["criteria"].as_object().unwrap();
    assert_eq!(
        criteria.keys().cloned().collect::<Vec<_>>(),
        ["forward-synthesis", "returning-inquiry"]
    );
    assert_eq!(sent[0]["state"]["subject"]["key"], "eros-verify");
    assert_eq!(sent[0]["state"]["chosen"]["vak-cf"][0]["label"], "CF3");
    // The ask is kept against the session that asked.
    let asks = world.aikit(&["decide", "asks", "--agent-session", ASKER]);
    assert_eq!(
        asks["data"]["asks"][0]["answers"]["vak-cs"]["choice"], "CS2",
        "{asks}"
    );
}

#[test]
fn a_cell_with_no_open_head_makes_no_call() {
    // Port 9 on loopback: nothing answers there, so a call would fail.
    let world = World::new("127.0.0.1:9");
    let filled = world.fill(
        "vak-block",
        json!({"vak-cpf": ["dialogical"], "vak-ct": ["CT2"], "vak-cp": ["4.2"], "vak-cf": ["CF3"],
        "vak-cfp": ["CFP2"], "vak-cs": ["CS2"], "vak-direction": ["returning-inquiry"]}),
    );
    assert_eq!(filled["ok"], true, "{filled}");
    assert_eq!(filled["data"]["outcome"], "nothing-open");
    assert_eq!(filled["data"]["calls"], 0);
    assert_eq!(filled["data"]["open"], json!([]));
    assert_eq!(
        world.aikit(&["decide", "asks", "--agent-session", ASKER])["data"]["asks"],
        json!([])
    );
}

#[test]
fn an_abstention_is_kept_as_given_and_not_asked_again() {
    let world = World::new("127.0.0.1:9");
    let filled = world.fill(
        "vak-block",
        json!({"vak-cpf": [], "vak-ct": ["CT2"], "vak-cp": ["4.2"], "vak-cf": ["CF3"],
        "vak-cfp": ["CFP2"], "vak-cs": ["CS2"], "vak-direction": ["returning-inquiry"]}),
    );
    assert_eq!(filled["data"]["outcome"], "nothing-open", "{filled}");
    assert_eq!(filled["data"]["chosen"]["vak-cpf"], json!([]));
}

#[test]
fn what_the_instrument_does_not_hold_is_refused_in_aikits_words() {
    let world = World::new("127.0.0.1:9");
    let wrong = world.fill("vak-block", json!({"vak-direction": ["returning"]}));
    assert_eq!(wrong["ok"], false, "{wrong}");
    assert_eq!(wrong["error"]["code"], "decision.instrument_chosen_invalid");
    assert!(
        wrong["error"]["message"]
            .as_str()
            .unwrap()
            .contains("forward-synthesis, returning-inquiry"),
        "{wrong}"
    );
    let unknown = world.fill("gate-9", json!({}));
    assert_eq!(
        unknown["error"]["code"], "decision.instrument_unknown",
        "{unknown}"
    );
    let person = world.fill("gate-6", json!({}));
    assert_eq!(
        person["error"]["code"], "decision.instrument_person_only",
        "{person}"
    );
    assert_eq!(
        world.aikit(&["decide", "asks", "--agent-session", ASKER])["data"]["asks"],
        json!([])
    );
}
