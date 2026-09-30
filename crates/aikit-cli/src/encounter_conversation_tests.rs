//! Conversation requests end to end: the real encounter owner with two resident
//! ACP provider processes, Central's real `ctrl`, and a real Flow file. Provider
//! replies are protocol FIXTURES derived from the recipient's name and the entry
//! it was asked, never model output; what is under test is correlation, the
//! owner-side incorporation into the Flow, recovery and disclosure.
//!
//! These tests need a built Central `ctrl`; set `AIKIT_TEST_CENTRAL_CTRL` to its
//! path (CI that has the suite's Central supplies it). Without it they say so
//! and pass vacuously rather than pretend.
#![cfg(unix)]
use super::conversation::spawn_worker;
use super::queue_tests::QueueWorld;
use crate::encounter_service::{
    ConversationEntry, ConversationRecipientSpec, ConversationSendRequest, EncounterRequest,
    EncounterService,
};
use aikit_core::ResourceRef;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

static ENV: Mutex<()> = Mutex::new(());

fn r(s: &str) -> ResourceRef {
    ResourceRef::parse(s).unwrap()
}
fn ctrl() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("AIKIT_TEST_CENTRAL_CTRL")?);
    path.exists().then_some(path)
}

struct Central {
    root: PathBuf,
    ctrl: PathBuf,
}
impl Central {
    fn new(base: &Path, ctrl: PathBuf) -> Self {
        let root = base.join("central");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let central = Self { root, ctrl };
        assert_eq!(central.action("central.init", json!({}))["ok"], true);
        std::fs::create_dir_all(central.root.join("Control/user/flows")).unwrap();
        central
    }
    fn action(&self, name: &str, input: Value) -> Value {
        let out = Command::new(&self.ctrl)
            .args(["--json", "--root"])
            .arg(&self.root)
            .args(["action", "run", name])
            .arg(input.to_string())
            .env_remove("CENTRAL_NATIVE_TOKEN")
            .output()
            .unwrap();
        serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|_| panic!("ctrl {name}: {}", String::from_utf8_lossy(&out.stderr)))
    }
    fn location(&self, name: &str) -> Value {
        let path = format!("Control/user/flows/{name}");
        json!({"schema":"central.path-ref/v1","ref":format!("central:path:{}:{path}", self.root.display()),"root":self.root.display().to_string(),"path":path})
    }
    fn create_flow(&self, name: &str, doc: &Value) -> Value {
        let loc = self.location(name);
        let html = format!(
            "<!doctype html><html><body><main id=\"app\"></main>\n<script type=\"application/json\" id=\"ql-doc\">{}</script></body></html>",
            serde_json::to_string(doc).unwrap().replace("</script", "<\\/script")
        );
        let made = self.action(
            "central.files.write",
            json!({"location":loc,"expected_revision":"","content":html,"actor":"human:desktop","actor_kind":"human"}),
        );
        assert_eq!(made["ok"], true, "{made}");
        loc
    }
    fn doc(&self, loc: &Value) -> Value {
        let read = self.action("central.files.read", json!({"location": loc}));
        let content = read["data"]["content"].as_str().unwrap();
        let start = content.find("id=\"ql-doc\">").unwrap() + "id=\"ql-doc\">".len();
        let end = content[start..].find("</script>").unwrap() + start;
        serde_json::from_str(&content[start..end].replace("<\\/script", "</script")).unwrap()
    }
}

fn flow_doc() -> Value {
    let person = json!({"key":"p-ann","initial":"A","kind":"person","name":"Ann","role":"contributor","binding":{"owner":"document","basis":"unknown"}});
    let agent = |key: &str, initial: &str, name: &str| json!({"key":key,"initial":initial,"kind":"agent","name":name,"role":"contributor","binding":{"owner":"document","basis":"unknown"}});
    json!({"meta":{"documentId":"doc-conv","created":"2026-09-30T08:00:00.000Z","title":"","revision":0,"view":"dialogue","current":null,"journalCurrent":null,"exported":null,
        "template":"ql-dialogue-flow v0.4","format":{"version":4,"minReader":4},"participants":[person,agent("p-ada","D","Ada"),agent("p-ash","S","Ash")]},
        "entries":[],"notes":[{"id":"n1","entryId":"x","text":"<p>PRIVATE NOTE</p>"}],"packet":[],"media":[],"journal":[{"id":"j1","at":"2026-09-30","html":"<p>PRIVATE JOURNAL</p>"}]})
}

struct Fixture {
    world: QueueWorld,
    central: Central,
    loc: Value,
    _env: std::sync::MutexGuard<'static, ()>,
}
impl Fixture {
    fn new(ctrl: PathBuf) -> Self {
        let env = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let world = QueueWorld::new();
        let central = Central::new(world._temp.path(), ctrl.clone());
        let loc = central.create_flow("flow-conv.html", &flow_doc());
        std::env::set_var("CENTRAL_CTRL_BIN", &ctrl);
        std::env::set_var("AIKIT_CENTRAL_ROOT", &central.root);
        std::env::remove_var("CENTRAL_NATIVE_TOKEN");
        Self {
            world,
            central,
            loc,
            _env: env,
        }
    }
    fn flow_ref(&self) -> String {
        self.loc["ref"].as_str().unwrap().to_owned()
    }
    fn send(
        &self,
        service: &EncounterService,
        request: &str,
        html: &str,
        recipients: &[(&str, &ResourceRef)],
    ) -> Result<Value, aikit_core::AikitError> {
        service.apply(EncounterRequest::ConversationSend {
            request: Box::new(ConversationSendRequest {
                request_ref: r(request),
                flow_location: self.loc.clone(),
                sender: r("human:ann"),
                actor: "human:ann".into(),
                actor_kind: "human".into(),
                author_session: None,
                entry: ConversationEntry {
                    author_key: "p-ann".into(),
                    html: html.into(),
                    at: "2026-09-30T09:00:00.000Z".into(),
                    relations: vec![],
                    addressees: vec![],
                    audience: None,
                    basis_revision: Some(0),
                },
                recipients: recipients
                    .iter()
                    .map(|(key, session)| ConversationRecipientSpec {
                        participant_key: (*key).into(),
                        agent_session: (*session).clone(),
                    })
                    .collect(),
            }),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["CENTRAL_CTRL_BIN", "AIKIT_CENTRAL_ROOT"] {
            std::env::remove_var(name);
        }
    }
}

fn states(service: &EncounterService, request: &str) -> Vec<(String, String)> {
    let reading = service
        .apply(EncounterRequest::ConversationRead {
            request_ref: r(request),
        })
        .unwrap();
    reading["recipients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["participant_key"].as_str().unwrap().to_owned(),
                x["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}
fn wait_for(
    service: &EncounterService,
    request: &str,
    want: &[(&str, &str)],
) -> Vec<(String, String)> {
    let deadline = Instant::now() + Duration::from_secs(40);
    loop {
        let now = states(service, request);
        if want
            .iter()
            .all(|(k, s)| now.iter().any(|(nk, ns)| nk == k && ns == s))
        {
            return now;
        }
        assert!(
            Instant::now() < deadline,
            "did not reach {want:?}; now {now:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn open_both(f: &Fixture, service: &EncounterService) -> (ResourceRef, ResourceRef) {
    let flow = f.flow_ref();
    let (space_a, ada) =
        f.world
            .attach_with("ada", &["human:ann"], &[&flow], "conversation_provider.py");
    let (space_s, ash) =
        f.world
            .attach_with("ash", &["human:ann"], &[&flow], "conversation_provider.py");
    f.world.open(service, &space_a, &ada, "ada");
    f.world.open(service, &space_s, &ash, "ash");
    (ada, ash)
}
macro_rules! need_ctrl {
    () => {
        match ctrl() {
            Some(path) => path,
            None => {
                eprintln!(
                    "AIKIT_TEST_CENTRAL_CTRL is not set: conversation end-to-end test not run"
                );
                return;
            }
        }
    };
}

#[test]
fn two_recipients_reply_independently_and_each_reply_lands_once_with_no_client_attached() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    let sent = f
        .send(
            &service,
            "conversation/q1",
            "<p>What does the passage claim?</p>",
            &[("p-ada", &ada), ("p-ash", &ash)],
        )
        .unwrap();
    assert_eq!(sent["fresh"], true);
    // From here nothing reads the conversation: no view, no watcher, no client.
    // The owner's worker alone carries both replies into the Flow.
    std::thread::sleep(Duration::from_millis(300));
    let done = wait_for(
        &service,
        "conversation/q1",
        &[("p-ada", "included"), ("p-ash", "included")],
    );
    assert_eq!(done.len(), 2);
    let doc = f.central.doc(&f.loc);
    let entries = doc["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        3,
        "the question and one answer each, no duplicates: {entries:#?}"
    );
    let question = &entries[0];
    assert_eq!(question["authorKey"], "p-ann");
    assert_eq!(question["intent"], "response");
    assert_eq!(question["addressees"], json!(["p-ada", "p-ash"]));
    let by = |key: &str| {
        entries
            .iter()
            .find(|e| e["authorKey"] == key && e["id"] != question["id"])
            .unwrap_or_else(|| panic!("no entry by {key}"))
    };
    let (a, s) = (by("p-ada"), by("p-ash"));
    assert!(
        a["html"]
            .as_str()
            .unwrap()
            .contains("Ada considers: What does the passage claim?"),
        "{a}"
    );
    assert!(
        s["html"]
            .as_str()
            .unwrap()
            .contains("Ash considers: What does the passage claim?"),
        "{s}"
    );
    assert!(
        !a["html"].as_str().unwrap().contains("Ash considers"),
        "no cross-contamination"
    );
    for (entry, session) in [(a, &ada), (s, &ash)] {
        assert_eq!(
            entry["replyTo"]["entryId"], question["id"],
            "each reply answers the asked entry"
        );
        assert_eq!(entry["relations"][0]["type"], "reply");
        assert_eq!(
            entry["basisRevision"], 1,
            "the reply keeps the asked entry's own document revision"
        );
        assert_eq!(entry["relations"][0]["revision"], 1);
        assert_eq!(entry["attribution"]["session"], session.as_str());
        assert_eq!(
            entry["attribution"]["basis"], "declared",
            "no host credential here, so never claimed verified"
        );
        assert_eq!(entry["intent"], "contribution");
    }
    // Private collections were never part of what a recipient was shown.
    for id in ["ada", "ash"] {
        let log = std::fs::read_to_string(f.world.cwd.join(format!("{id}.log"))).unwrap();
        assert!(
            !log.contains("PRIVATE NOTE") && !log.contains("PRIVATE JOURNAL"),
            "a recipient must not see private collections"
        );
        assert!(log.contains("You are "), "the prompt names the recipient");
        assert!(log.contains("ASKED [1] Ann (person): What does the passage claim?"));
    }
    // A second reconcile finds nothing left to do and changes nothing.
    let before = f.central.doc(&f.loc)["meta"]["revision"].clone();
    service
        .apply(EncounterRequest::ConversationReconcile {
            request_ref: r("conversation/q1"),
        })
        .unwrap();
    assert_eq!(f.central.doc(&f.loc)["meta"]["revision"], before);
    let reading = service
        .apply(EncounterRequest::ConversationRead {
            request_ref: r("conversation/q1"),
        })
        .unwrap();
    assert_eq!(reading["task_completion"], "not-inferred");
    assert_eq!(reading["recipients"][0]["delivery"]["phase"], "returned");
}

#[test]
fn a_restarted_owner_finishes_what_the_first_one_left() {
    let f = Fixture::new(need_ctrl!());
    let (ada, ash) = {
        // First owner: no worker. It records, commits the entry and dispatches;
        // the recipients answer into the journal; then the owner goes away
        // before anything reads or incorporates a reply.
        let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
        let (ada, ash) = open_both(&f, &service);
        f.send(
            &service,
            "conversation/q2",
            "<p>Does the argument survive a counterexample?</p>",
            &[("p-ada", &ada), ("p-ash", &ash)],
        )
        .unwrap();
        wait_for(
            &service,
            "conversation/q2",
            &[("p-ada", "returned"), ("p-ash", "returned")],
        );
        assert_eq!(
            f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
            1,
            "only the question is in the Flow: nothing has incorporated a reply"
        );
        (ada, ash)
    };
    // A fresh owner with a worker: same durable state, same remaining work.
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    spawn_worker(&service);
    wait_for(
        &service,
        "conversation/q2",
        &[("p-ada", "included"), ("p-ash", "included")],
    );
    let doc = f.central.doc(&f.loc);
    assert_eq!(doc["entries"].as_array().unwrap().len(), 3);
    let mut keys: Vec<&str> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["authorKey"].as_str().unwrap())
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        ["p-ada", "p-ann", "p-ash"],
        "each reply landed exactly once, none lost to the restart"
    );
    let _ = (ada, ash);
}

#[test]
fn a_busy_recipient_holds_its_turn_and_a_refused_one_is_named_without_dropping_its_sibling() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let flow = f.flow_ref();
    let (space_a, ada) =
        f.world
            .attach_with("ada", &["human:ann"], &[&flow], "conversation_provider.py");
    f.world.open(&service, &space_a, &ada, "ada");
    // Ash exists but does not admit this sender: its disclosure is its own.
    let (space_s, ash) = f.world.attach_with(
        "ash",
        &["human:someone-else"],
        &[&flow],
        "conversation_provider.py",
    );
    f.world.open(&service, &space_s, &ash, "ash");
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/slow",
        "<p>Take your time. SLOW_50</p>",
        &[("p-ada", &ada)],
    )
    .unwrap();
    // A second request arrives for the same session while the first is in flight.
    let second = f
        .send(
            &service,
            "conversation/next",
            "<p>And the second question?</p>",
            &[("p-ada", &ada), ("p-ash", &ash)],
        )
        .unwrap();
    let held = second["request"]["recipients"].as_array().unwrap();
    let ada_now = held
        .iter()
        .find(|x| x["participant_key"] == "p-ada")
        .unwrap();
    assert!(
        matches!(
            ada_now["state"].as_str().unwrap(),
            "held" | "waiting-for-entry"
        ),
        "a session serves one delivery at a time: {ada_now}"
    );
    wait_for(&service, "conversation/slow", &[("p-ada", "included")]);
    // The busy recipient's turn comes once the first delivery resolves; the
    // sibling refused at admission is named with its code and never included.
    let done = wait_for(
        &service,
        "conversation/next",
        &[("p-ada", "included"), ("p-ash", "refused")],
    );
    assert_eq!(done.len(), 2);
    let reading = service
        .apply(EncounterRequest::ConversationRead {
            request_ref: r("conversation/next"),
        })
        .unwrap();
    let ash_reading = reading["recipients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["participant_key"] == "p-ash")
        .unwrap();
    assert!(
        ash_reading["dispatch"]["detail"]
            .as_str()
            .unwrap()
            .contains("encounter.disclosure_denied"),
        "{ash_reading}"
    );
    let doc = f.central.doc(&f.loc);
    let ada_entries = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["authorKey"] == "p-ada")
        .count();
    assert_eq!(ada_entries, 2, "Ada answered each question once");
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        0,
        "a refused recipient contributes nothing"
    );
    let log = std::fs::read_to_string(f.world.cwd.join("ash.log")).unwrap_or_default();
    assert!(
        !log.contains("session/prompt"),
        "a refused recipient was never prompted"
    );
}

#[test]
fn a_replayed_request_is_recognised_and_a_changed_one_under_the_same_identity_is_refused() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, _ash) = open_both(&f, &service);
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/once",
        "<p>First asked</p>",
        &[("p-ada", &ada)],
    )
    .unwrap();
    wait_for(&service, "conversation/once", &[("p-ada", "included")]);
    let entries_after = f.central.doc(&f.loc)["entries"].as_array().unwrap().len();
    let replay = f
        .send(
            &service,
            "conversation/once",
            "<p>First asked</p>",
            &[("p-ada", &ada)],
        )
        .unwrap();
    assert_eq!(replay["fresh"], false, "the same request is a replay");
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(
        f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
        entries_after,
        "a replay adds nothing and re-prompts no one"
    );
    let changed = f
        .send(
            &service,
            "conversation/once",
            "<p>A different question</p>",
            &[("p-ada", &ada)],
        )
        .unwrap_err();
    assert_eq!(changed.code(), "conversation.request_conflict");
    assert_eq!(
        f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
        entries_after,
        "a refused change writes nothing"
    );
}
