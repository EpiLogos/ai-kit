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
    /// Change the Flow the way a person editing it would: read it, change its
    /// document, write it back at the revision that was read.
    fn edit_doc(&self, loc: &Value, change: impl FnOnce(&mut Value)) {
        let read = self.action("central.files.read", json!({"location": loc}));
        let content = read["data"]["content"].as_str().unwrap().to_owned();
        let open = "id=\"ql-doc\">";
        let start = content.find(open).unwrap() + open.len();
        let end = content[start..].find("</script>").unwrap() + start;
        let mut doc: Value =
            serde_json::from_str(&content[start..end].replace("<\\/script", "</script")).unwrap();
        change(&mut doc);
        let html = format!(
            "{}{}{}",
            &content[..start],
            serde_json::to_string(&doc)
                .unwrap()
                .replace("</script", "<\\/script"),
            &content[end..]
        );
        let written = self.action(
            "central.files.write",
            json!({"location":loc,"expected_revision":read["data"]["revision"],"content":html,"actor":"human:desktop","actor_kind":"human"}),
        );
        assert_eq!(written["ok"], true, "{written}");
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
        Self::with_doc(ctrl, flow_doc())
    }
    fn with_doc(ctrl: PathBuf, doc: Value) -> Self {
        let env = ENV.lock().unwrap_or_else(|p| p.into_inner());
        let world = QueueWorld::new();
        let central = Central::new(world._temp.path(), ctrl.clone());
        let loc = central.create_flow("flow-conv.html", &doc);
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
                        route: None,
                        agent_ref: None,
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
        "<p>Take your time. SLOW_100</p>",
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

#[test]
fn a_crash_before_any_effect_or_between_the_entry_commit_and_its_record_is_finished_by_the_next_owner(
) {
    let f = Fixture::new(need_ctrl!());
    let flow = f.flow_ref();
    let (space_a, ada) =
        f.world
            .attach_with("ada", &["human:ann"], &[&flow], "conversation_provider.py");
    // Boundary 1 — the request was recorded, then the owner died before anything happened.
    {
        let service = EncounterService::new(f.world.home.clone()).unwrap();
        f.world.open(&service, &space_a, &ada, "ada");
        let body = json!({"schema":"aikit.conversation-request/v1","flow":{"location":f.loc},"sender":"human:ann","actor":"human:ann","actor_kind":"human",
            "entry":{"author_key":"p-ann","html":"<p>Recorded, then the owner died.</p>","at":"2026-09-30T09:00:00.000Z","relations":[],"addressees":[],"basis_revision":0}});
        service
            .store
            .create_conversation(
                &r("conversation/crash1"),
                "digest-1",
                &body,
                &[aikit_store::encounter::NewConversationRecipient {
                    participant_key: "p-ada".into(),
                    agent_session: ada.clone(),
                    delivery_ref: r("delivery/conv-crash1-ada"),
                    agent_ref: None,
                    route: None,
                }],
            )
            .unwrap();
        assert_eq!(
            f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
            0,
            "nothing has happened in the Flow"
        );
    }
    // Boundary 2 — the entry reached the Flow (Central committed it) but the owner never noted that.
    let committed = f.central.action("central.flow.append", json!({
        "location": f.loc, "operation_ref": "conv-entry:conversation/crash1", "author_key": "p-ann", "html": "<p>Recorded, then the owner died.</p>",
        "at": "2026-09-30T09:00:00.000Z", "addressees": ["p-ada"], "intent": "response", "relations": [], "basis_revision": 0,
        "actor": "human:ann", "actor_kind": "human"}));
    assert_eq!(committed["ok"], true, "{committed}");
    assert_eq!(
        f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
        1
    );
    // A fresh owner with a worker completes it: the same entry is recovered, never doubled, and Ada is asked once.
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    // A restarted owner resumes the recorded session explicitly; it never opens it afresh.
    service
        .apply(EncounterRequest::Reconnect {
            space: space_a.clone(),
            agent_session: ada.clone(),
            provider: "ada".into(),
            cwd: f.world.cwd.clone(),
        })
        .map_err(|failure| failure.to_string())
        .unwrap();
    spawn_worker(&service);
    wait_for(&service, "conversation/crash1", &[("p-ada", "included")]);
    let doc = f.central.doc(&f.loc);
    let entries = doc["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        2,
        "the entry once, Ada's answer once: {entries:#?}"
    );
    assert_eq!(
        entries.iter().filter(|e| e["authorKey"] == "p-ann").count(),
        1,
        "the authored entry was recovered, not doubled"
    );
    let prompts = std::fs::read_to_string(f.world.cwd.join("ada.log"))
        .unwrap()
        .matches("session/prompt")
        .count();
    assert_eq!(prompts, 1, "the recipient was asked exactly once");
}

#[test]
fn a_crash_after_the_reply_reached_the_flow_but_before_the_owner_noted_it_recovers_the_same_entry()
{
    let f = Fixture::new(need_ctrl!());
    let (ada, ash) = {
        let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
        let (ada, ash) = open_both(&f, &service);
        f.send(
            &service,
            "conversation/crash2",
            "<p>Is the claim sound?</p>",
            &[("p-ada", &ada), ("p-ash", &ash)],
        )
        .unwrap();
        wait_for(
            &service,
            "conversation/crash2",
            &[("p-ada", "returned"), ("p-ash", "returned")],
        );
        (ada, ash)
    };
    // The owner appended Ada's reply to the Flow and died before recording it: do exactly that step, with the
    // exact payload the owner builds, through Central.
    let service = EncounterService::new(f.world.home.clone()).unwrap();
    let reading = service
        .apply(EncounterRequest::ConversationRead {
            request_ref: r("conversation/crash2"),
        })
        .unwrap();
    let ada_row = reading["recipients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["participant_key"] == "p-ada")
        .unwrap();
    let text = ada_row["reply"]["text"].as_str().unwrap().to_owned();
    let entry = &reading["entry"];
    let appended = f.central.action("central.flow.append", json!({
        "location": f.loc, "operation_ref": "conv-reply:conversation/crash2:p-ada", "author_key": "p-ada", "html": format!("<p>{text}</p>"),
        "at": "2026-09-30T09:30:00.000Z",
        "relations": [{"type":"reply","entryId": entry["entry_id"], "revision": entry["document_revision"], "anchor": null}],
        "addressees": ["p-ann"], "intent": "contribution", "basis_revision": entry["document_revision"],
        "actor": ada.as_str(), "actor_kind": "agent", "agent_session_ref": ada.as_str(), "agent_ref": "agent:ada"}));
    assert_eq!(appended["ok"], true, "{appended}");
    let before = f.central.doc(&f.loc)["entries"].as_array().unwrap().len();
    let service = Arc::new(service);
    spawn_worker(&service);
    wait_for(
        &service,
        "conversation/crash2",
        &[("p-ada", "included"), ("p-ash", "included")],
    );
    let doc = f.central.doc(&f.loc);
    let adas: Vec<&Value> = doc["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["authorKey"] == "p-ada")
        .collect();
    assert_eq!(
        adas.len(),
        1,
        "Ada's reply is in the Flow once, recovered not doubled"
    );
    assert_eq!(
        doc["entries"].as_array().unwrap().len(),
        before + 1,
        "only Ash's answer was added"
    );
    let _ = ash;
}

// ---------------------------------------------------------------------------
// Repairs found by independent verification of the plural Flow (O:I #558):
// which seat a session may answer as, who may still be asked, a turn ending on
// a session that queued a recipient, and a delivery an owner died holding.
// ---------------------------------------------------------------------------

fn seat<'a>(doc: &'a mut Value, key: &str) -> &'a mut Value {
    doc["meta"]["participants"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["key"] == key)
        .unwrap_or_else(|| panic!("no participant {key}"))
}
fn prompt_count(f: &Fixture, id: &str) -> usize {
    std::fs::read_to_string(f.world.cwd.join(format!("{id}.log")))
        .unwrap_or_default()
        .matches("session/prompt")
        .count()
}
fn recipient_of(service: &EncounterService, request: &str, key: &str) -> Value {
    service
        .apply(EncounterRequest::ConversationRead {
            request_ref: r(request),
        })
        .unwrap()["recipients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["participant_key"] == key)
        .unwrap_or_else(|| panic!("no recipient {key}"))
        .clone()
}
/// A turn of the session's own — what its composer starts — not a conversation delivery.
fn start_composer_turn(service: &EncounterService, session: &ResourceRef, text: &str) {
    let draft = service
        .apply(EncounterRequest::Draft {
            agent_session: session.clone(),
            basis: 0,
            text: text.into(),
        })
        .unwrap();
    service
        .apply(EncounterRequest::Prompt {
            agent_session: session.clone(),
            draft_revision: draft["revision"].as_u64().unwrap(),
        })
        .unwrap();
}
fn refused_with(service: &EncounterService, request: &str, key: &str, code: &str) -> Value {
    let recipient = wait_for(service, request, &[(key, "refused")]);
    assert!(recipient.iter().any(|(k, s)| k == key && s == "refused"));
    let recipient = recipient_of(service, request, key);
    assert!(
        recipient["dispatch"]["detail"]
            .as_str()
            .unwrap_or_default()
            .contains(code),
        "the standing names why ({code}): {recipient}"
    );
    recipient
}

fn facts(participants: Value, entries: &[&str]) -> super::conversation::FlowFacts {
    super::conversation::FlowFacts {
        participants: participants.as_array().unwrap().clone(),
        entry_ids: entries.iter().map(|e| (*e).to_owned()).collect(),
    }
}
fn check(
    facts: &super::conversation::FlowFacts,
    key: &str,
    session: &str,
    agent: Option<&str>,
    strict: bool,
    asked: Option<i64>,
    membership: bool,
) -> Result<(), &'static str> {
    super::conversation::seat_check(facts, key, &r(session), agent, strict, asked, membership)
        .map_err(|refusal| refusal.code)
}

#[test]
fn a_seat_is_asked_only_as_the_session_and_agent_it_declares() {
    // Pure: no Central needed. The Flow declares each seat; a request is carried
    // only to a seat whose declaration it matches.
    let seats = json!([
        {"key":"p-cy","kind":"agent","name":"Cy","role":"contributor","binding":{"owner":"central","ref":"agent/cy","basis":"declared"},"ref":"agent-session/cy"},
        {"key":"p-free","kind":"agent","name":"Free","role":"contributor","binding":{"owner":"document","basis":"unknown","ref":""}},
        {"key":"p-ann","kind":"person","name":"Ann","role":"contributor","binding":{"owner":"document","basis":"unknown"}},
        {"key":"p-gone","kind":"agent","name":"Gone","role":"contributor","left":{"at":"x","revision":3}},
        {"key":"p-look","kind":"agent","name":"Look","role":"observer"},
        {"key":"p-hor","kind":"agent","name":"Hor","role":"contributor","historyFrom":"e-missing"},
        {"key":"p-late","kind":"agent","name":"Late","role":"contributor","joined":{"at":"x","revision":4}},
    ]);
    let f = facts(seats, &["e-1"]);
    let ok = |key, session, agent| check(&f, key, session, agent, true, Some(4), true);
    assert_eq!(ok("p-cy", "agent-session/cy", Some("agent/cy")), Ok(()));
    // Another agent's session is never carried to Cy's seat.
    assert_eq!(
        ok("p-cy", "agent-session/cy", Some("agent/ash")),
        Err("conversation.seat_bound_to_another_agent")
    );
    // The seat names the session that answers from it.
    assert_eq!(
        ok("p-cy", "agent-session/ash", Some("agent/cy")),
        Err("conversation.seat_session_mismatch")
    );
    // An unbound seat needs the request to prove which agent answers.
    assert_eq!(
        ok("p-free", "agent-session/any", None),
        Err("conversation.seat_unproven")
    );
    assert_eq!(ok("p-free", "agent-session/any", Some("agent/x")), Ok(()));
    assert_eq!(
        check(&f, "p-free", "agent-session/any", None, false, None, true),
        Ok(()),
        "a local recipient's proof is the owner's own reading, settled at dispatch"
    );
    // Who may still be asked.
    assert_eq!(
        ok("p-ann", "agent-session/a", Some("agent/a")),
        Err("conversation.recipient_not_agent")
    );
    assert_eq!(
        ok("p-gone", "agent-session/a", Some("agent/a")),
        Err("conversation.participant_left")
    );
    assert_eq!(
        ok("p-look", "agent-session/a", Some("agent/a")),
        Err("conversation.observer_cannot_contribute")
    );
    assert_eq!(
        ok("p-hor", "agent-session/a", Some("agent/a")),
        Err("conversation.history_unresolved")
    );
    assert_eq!(
        ok("p-late", "agent-session/a", Some("agent/a")),
        Err("conversation.joined_after_entry")
    );
    assert_eq!(
        ok("p-nobody", "agent-session/a", Some("agent/a")),
        Err("conversation.not_a_participant")
    );
    // Before the entry is committed only the structural facts are judged: the
    // rest is judged per recipient at dispatch, so a departed participant does
    // not fail their siblings' request.
    assert_eq!(
        check(
            &f,
            "p-gone",
            "agent-session/a",
            Some("agent/a"),
            false,
            None,
            false
        ),
        Ok(())
    );
    assert_eq!(
        check(&f, "p-nobody", "agent-session/a", None, false, None, false),
        Err("conversation.not_a_participant")
    );
}

#[test]
fn a_request_routed_to_the_wrong_agents_session_is_refused_before_anything_is_recorded_or_asked() {
    let ctrl = need_ctrl!();
    let mut doc = flow_doc();
    // Ash's seat is declared for another agent; Ada's names another session.
    *seat(&mut doc, "p-ash") = json!({"key":"p-ash","initial":"S","kind":"agent","name":"Ash","role":"contributor","binding":{"owner":"central","ref":"agent:cy-nobody","basis":"declared"}});
    seat(&mut doc, "p-ada")["ref"] = json!("agent-session/somebody-else");
    let f = Fixture::with_doc(ctrl, doc);
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, ash) = open_both(&f, &service);
    spawn_worker(&service);

    // Ash's session is agent:ash; the seat says agent:cy-nobody.
    let wrong_agent = f
        .send(
            &service,
            "conversation/seat-a",
            "<p>Cy?</p>",
            &[("p-ash", &ash)],
        )
        .unwrap_err();
    assert_eq!(
        wrong_agent.code(),
        "conversation.seat_bound_to_another_agent"
    );
    // Ada's session is not the one her seat answers from.
    let wrong_session = f
        .send(
            &service,
            "conversation/seat-b",
            "<p>Ada?</p>",
            &[("p-ada", &ada)],
        )
        .unwrap_err();
    assert_eq!(wrong_session.code(), "conversation.seat_session_mismatch");
    // A sibling that is fine does not rescue the request: it is refused whole.
    let mixed = f
        .send(
            &service,
            "conversation/seat-c",
            "<p>Both?</p>",
            &[("p-ada", &ada), ("p-ash", &ash)],
        )
        .unwrap_err();
    assert!(
        mixed.code().starts_with("conversation.seat_"),
        "{}",
        mixed.code()
    );
    std::thread::sleep(Duration::from_millis(600));
    for request in [
        "conversation/seat-a",
        "conversation/seat-b",
        "conversation/seat-c",
    ] {
        assert_eq!(
            service
                .apply(EncounterRequest::ConversationRead {
                    request_ref: r(request)
                })
                .unwrap_err()
                .code(),
            "conversation.unknown",
            "nothing was recorded"
        );
    }
    assert_eq!(
        f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
        0,
        "nothing reached the Flow"
    );
    assert_eq!(
        prompt_count(&f, "ash") + prompt_count(&f, "ada"),
        0,
        "no model was asked"
    );
    // The seat is not rebound by a request that was refused.
    let after = f.central.doc(&f.loc);
    let ash_after = after["meta"]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "p-ash")
        .unwrap()
        .clone();
    assert_eq!(ash_after["binding"]["ref"], "agent:cy-nobody");
}

#[test]
fn a_seat_that_matches_its_session_and_agent_is_asked_and_stays_bound_to_that_agent() {
    let ctrl = need_ctrl!();
    let mut doc = flow_doc();
    seat(&mut doc, "p-ada")["binding"] =
        json!({"owner":"central","ref":"agent:ada","basis":"declared"});
    let f = Fixture::with_doc(ctrl, doc);
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, _ash) = open_both(&f, &service);
    f.central.edit_doc(&f.loc, |doc| {
        seat(doc, "p-ada")["ref"] = json!(ada.as_str())
    });
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/seat-ok",
        "<p>Ada, yes?</p>",
        &[("p-ada", &ada)],
    )
    .unwrap();
    wait_for(&service, "conversation/seat-ok", &[("p-ada", "included")]);
    let doc = f.central.doc(&f.loc);
    let ada_seat = doc["meta"]["participants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == "p-ada")
        .unwrap();
    assert_eq!(
        ada_seat["binding"]["ref"], "agent:ada",
        "the seat stays declared for the agent it declared"
    );
    assert_eq!(ada_seat["ref"], ada.as_str());
}

#[test]
fn an_unbound_seat_on_another_workcell_is_carried_only_with_the_agent_the_request_names() {
    let ctrl = need_ctrl!();
    let f = Fixture::new(ctrl);
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let route =
        json!({"kind":"exec","aikit":"/bin/false","cwd":"/tmp","workcell":"workcell:elsewhere"});
    let send = |request: &str, agent_ref: Option<&str>| {
        service.apply(EncounterRequest::ConversationSend {
            request: Box::new(ConversationSendRequest {
                request_ref: r(request),
                flow_location: f.loc.clone(),
                sender: r("human:ann"),
                actor: "human:ann".into(),
                actor_kind: "human".into(),
                author_session: None,
                entry: ConversationEntry {
                    author_key: "p-ann".into(),
                    html: "<p>Far away?</p>".into(),
                    at: "2026-09-30T09:00:00.000Z".into(),
                    relations: vec![],
                    addressees: vec![],
                    audience: None,
                    basis_revision: Some(0),
                },
                recipients: vec![ConversationRecipientSpec {
                    participant_key: "p-ada".into(),
                    agent_session: r("agent-session/far"),
                    route: Some(route.clone()),
                    agent_ref: agent_ref.map(str::to_owned),
                }],
            }),
        })
    };
    assert_eq!(
        send("conversation/far-a", None).unwrap_err().code(),
        "conversation.seat_unproven",
        "nothing proves which agent that session is"
    );
    assert_eq!(
        f.central.doc(&f.loc)["entries"].as_array().unwrap().len(),
        0
    );
    // With the agent named, the request is accepted (the other owner is not there,
    // so the recipient is held, and the reason says so).
    let sent = send("conversation/far-b", Some("agent/far")).unwrap();
    assert_eq!(sent["fresh"], true);
}

#[test]
fn a_participant_who_has_left_is_refused_without_a_turn_and_their_sibling_still_answers() {
    let ctrl = need_ctrl!();
    let mut doc = flow_doc();
    seat(&mut doc, "p-ash")["left"] = json!({"at":"2026-09-30T08:30:00.000Z","revision":0});
    let f = Fixture::with_doc(ctrl, doc);
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/left",
        "<p>Anyone there?</p>",
        &[("p-ada", &ada), ("p-ash", &ash)],
    )
    .unwrap();
    let gone = refused_with(
        &service,
        "conversation/left",
        "p-ash",
        "conversation.participant_left",
    );
    assert_eq!(gone["state"], "refused");
    wait_for(&service, "conversation/left", &[("p-ada", "included")]);
    assert_eq!(
        prompt_count(&f, "ash"),
        0,
        "the model of a departed participant is never run"
    );
    assert_eq!(prompt_count(&f, "ada"), 1);
    let doc = f.central.doc(&f.loc);
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        0
    );
}

#[test]
fn a_participant_whose_history_horizon_or_join_cannot_be_reconciled_with_the_entry_is_refused() {
    let ctrl = need_ctrl!();
    let mut doc = flow_doc();
    seat(&mut doc, "p-ash")["historyFrom"] = json!("e-no-such-entry");
    seat(&mut doc, "p-ada")["joined"] = json!({"at":"2026-09-30T08:59:00.000Z","revision":9});
    let f = Fixture::with_doc(ctrl, doc);
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/horizon",
        "<p>Can you read this?</p>",
        &[("p-ada", &ada), ("p-ash", &ash)],
    )
    .unwrap();
    refused_with(
        &service,
        "conversation/horizon",
        "p-ash",
        "conversation.history_unresolved",
    );
    refused_with(
        &service,
        "conversation/horizon",
        "p-ada",
        "conversation.joined_after_entry",
    );
    assert_eq!(
        prompt_count(&f, "ash") + prompt_count(&f, "ada"),
        0,
        "neither was asked"
    );
}

#[test]
fn a_recipient_queued_behind_a_composer_turn_is_dispatched_when_that_turn_ends() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (_ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    // A turn of Ash's own (not a conversation delivery) is in flight.
    start_composer_turn(&service, &ash, "An unrelated private chat. SLOW_100");
    f.send(
        &service,
        "conversation/behind",
        "<p>When you are free: your view?</p>",
        &[("p-ash", &ash)],
    )
    .unwrap();
    let waiting = recipient_of(&service, "conversation/behind", "p-ash");
    assert_eq!(waiting["state"], "queued", "{waiting}");
    // Nothing reopens the session and no client asks again: the end of the busy
    // turn is what dispatches it, and the reply is included.
    wait_for(&service, "conversation/behind", &[("p-ash", "included")]);
    assert_eq!(
        prompt_count(&f, "ash"),
        2,
        "the composer turn, then exactly one conversation turn"
    );
    let doc = f.central.doc(&f.loc);
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        1
    );
}

#[test]
fn a_participant_who_leaves_while_queued_is_revalidated_at_dispatch_and_never_run() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (_ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    start_composer_turn(&service, &ash, "An unrelated private chat. SLOW_100");
    f.send(
        &service,
        "conversation/leaves",
        "<p>Your view?</p>",
        &[("p-ash", &ash)],
    )
    .unwrap();
    assert_eq!(
        recipient_of(&service, "conversation/leaves", "p-ash")["state"],
        "queued"
    );
    // While it waits, Ash leaves the conversation.
    f.central.edit_doc(&f.loc, |doc| {
        seat(doc, "p-ash")["left"] = json!({"at":"2026-09-30T09:05:00.000Z","revision":2});
    });
    let gone = refused_with(
        &service,
        "conversation/leaves",
        "p-ash",
        "conversation.participant_left",
    );
    assert_eq!(gone["state"], "refused");
    std::thread::sleep(Duration::from_millis(800));
    assert_eq!(
        prompt_count(&f, "ash"),
        1,
        "only the composer turn ever ran: the queued work was not"
    );
    let row = service
        .store
        .delivery(&ash, &r(gone["delivery_ref"].as_str().unwrap()))
        .unwrap()
        .unwrap();
    assert_eq!(
        row.phase, "failed",
        "the queue row is closed, releasing the session"
    );
    assert!(service.store.queued_deliveries(&ash).unwrap().is_empty());
    let doc = f.central.doc(&f.loc);
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        0
    );
}

#[test]
fn a_participant_who_leaves_while_their_turn_runs_is_not_included() {
    let f = Fixture::new(need_ctrl!());
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    let (_ada, ash) = open_both(&f, &service);
    spawn_worker(&service);
    f.send(
        &service,
        "conversation/mid",
        "<p>Think about it. SLOW_60</p>",
        &[("p-ash", &ash)],
    )
    .unwrap();
    wait_for(&service, "conversation/mid", &[("p-ash", "delivered")]);
    f.central.edit_doc(&f.loc, |doc| {
        seat(doc, "p-ash")["left"] = json!({"at":"2026-09-30T09:05:00.000Z","revision":2});
    });
    wait_for(
        &service,
        "conversation/mid",
        &[("p-ash", "returned-not-included")],
    );
    let ash_now = recipient_of(&service, "conversation/mid", "p-ash");
    assert!(
        ash_now["inclusion"]["detail"]
            .as_str()
            .unwrap()
            .contains("conversation.participant_left"),
        "{ash_now}"
    );
    let doc = f.central.doc(&f.loc);
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        0
    );
}

#[test]
fn a_delivery_the_owner_died_holding_is_named_uncertain_released_and_never_replayed() {
    let f = Fixture::new(need_ctrl!());
    let flow = f.flow_ref();
    let (space_a, ada) =
        f.world
            .attach_with("ada", &["human:ann"], &[&flow], "conversation_provider.py");
    let (_space_s, ash) =
        f.world
            .attach_with("ash", &["human:ann"], &[&flow], "conversation_provider.py");
    let request = r("conversation/died");
    let (ada_delivery, ash_delivery) = (r("delivery/conv-died-ada"), r("delivery/conv-died-ash"));
    {
        // The first owner: it recorded the request, committed the entry, and sent
        // both recipients. Ada's turn was streaming when it died; Ash's had ended.
        let service = EncounterService::new(f.world.home.clone()).unwrap();
        f.world.open(&service, &space_a, &ada, "ada");
        let body = json!({"schema":"aikit.conversation-request/v1","flow":{"location":f.loc},"sender":"human:ann","actor":"human:ann","actor_kind":"human",
            "entry":{"author_key":"p-ann","html":"<p>Is the claim sound?</p>","at":"2026-09-30T09:00:00.000Z","relations":[],"addressees":[],"basis_revision":0}});
        let recipient = |key: &str, session: &ResourceRef, delivery: &ResourceRef| {
            aikit_store::encounter::NewConversationRecipient {
                participant_key: key.into(),
                agent_session: session.clone(),
                delivery_ref: delivery.clone(),
                agent_ref: None,
                route: None,
            }
        };
        service
            .store
            .create_conversation(
                &request,
                "digest-died",
                &body,
                &[
                    recipient("p-ada", &ada, &ada_delivery),
                    recipient("p-ash", &ash, &ash_delivery),
                ],
            )
            .unwrap();
        let committed = f.central.action("central.flow.append", json!({
            "location": f.loc, "operation_ref": format!("conv-entry:{request}"), "author_key": "p-ann", "html": "<p>Is the claim sound?</p>",
            "at": "2026-09-30T09:00:00.000Z", "addressees": ["p-ada","p-ash"], "intent": "response", "relations": [], "basis_revision": 0,
            "actor": "human:ann", "actor_kind": "human"}));
        assert_eq!(committed["ok"], true, "{committed}");
        service.store.conversation_record_source(&request, &json!({"entry_id": committed["data"]["entry"]["id"], "revision": committed["data"]["revision"], "document_revision": committed["data"]["document_revision"]})).unwrap();
        for (session, delivery, key, generation) in [
            (&ada, &ada_delivery, "p-ada", "gen-dead-ada"),
            (&ash, &ash_delivery, "p-ash", "gen-dead-ash"),
        ] {
            service
                .store
                .reserve_delivery(
                    session,
                    delivery,
                    &r("human:ann"),
                    &json!({"connection_generation": generation}),
                )
                .unwrap();
            service
                .store
                .delivery_ack(session, delivery, true, None)
                .unwrap();
            service
                .store
                .conversation_set_dispatch(&request, key, "sent", None)
                .unwrap();
            service.store.append(session, &json!({"kind":"provider","connection_generation":generation,"event":{"Signal":{"sequence":1,"native_session_id":"native-x","kind":{"kind":"agent-message-chunk","text":format!("{key} was saying: ")}}}})).unwrap();
        }
        // Ash's turn reached its end in the journal before the owner died.
        service.store.append(&ash, &json!({"kind":"provider","connection_generation":"gen-dead-ash","event":{"TurnEnded":{"stop":{"Completed":"EndTurn"}}}})).unwrap();
        let ada_before = recipient_of(&service, "conversation/died", "p-ada");
        assert_eq!(
            ada_before["state"], "answering",
            "the first owner still reads it as in flight: {ada_before}"
        );
    }
    // A fresh owner. Nothing here carries the dead generation.
    let service = Arc::new(EncounterService::new(f.world.home.clone()).unwrap());
    service
        .apply(EncounterRequest::Reconnect {
            space: space_a.clone(),
            agent_session: ada.clone(),
            provider: "ada".into(),
            cwd: f.world.cwd.clone(),
        })
        .map_err(|failure| failure.to_string())
        .unwrap();
    spawn_worker(&service);
    // Ash's terminal event is in the journal: its reply is included.
    wait_for(&service, "conversation/died", &[("p-ash", "included")]);
    // Ada's is not: unknown, with the exact continuation it came from — and no replay.
    let ada_now = recipient_of(&service, "conversation/died", "p-ada");
    assert_eq!(ada_now["state"], "uncertain", "{ada_now}");
    let detail = ada_now["delivery"]["detail"].as_str().unwrap();
    for named in [
        "gen-dead-ada",
        "native-x",
        "agent-session/ada",
        "not replayed",
    ] {
        assert!(
            detail.contains(named),
            "the continuation names {named}: {detail}"
        );
    }
    assert_eq!(
        prompt_count(&f, "ada"),
        0,
        "the lost turn was never dispatched again"
    );
    // Ada's session is released: a new request is asked and answered.
    f.send(
        &service,
        "conversation/after",
        "<p>And now?</p>",
        &[("p-ada", &ada)],
    )
    .unwrap();
    wait_for(&service, "conversation/after", &[("p-ada", "included")]);
    assert_eq!(
        prompt_count(&f, "ada"),
        1,
        "exactly the new request was asked"
    );
    assert_eq!(
        recipient_of(&service, "conversation/died", "p-ada")["state"],
        "uncertain",
        "and the lost one stays named, not retried"
    );
    let doc = f.central.doc(&f.loc);
    assert_eq!(
        doc["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["authorKey"] == "p-ash")
            .count(),
        1
    );
}
