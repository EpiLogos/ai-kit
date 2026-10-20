//! The Day's agent runs a move, proven in a disposable Central root with the
//! real `ctrl` (the Day's moves Actions, Central PR #286) and, for the end-to-end
//! cases, the real `aikit` binary, the real resident encounter owner and AIKit's
//! own controlled protocol peer (python, as `tests/agent_session_protocol.rs`
//! uses: a peer, not a model).
//!
//! The root here is built for the test: a civil-time policy, a placement policy,
//! and a native-action-authority naming one human and one native-service
//! principal (`native-service:aikit/day-agent`), whose grants are exactly the
//! Actions a real run needs. Nothing touches the owner's Central or AIKit home.
//!
//! The real `ctrl` is `$AIKIT_TEST_CTRL` or the one on PATH, and it must carry
//! the Day's moves Actions. Without one the test says so and skips;
//! `AIKIT_REQUIRE_NATIVE_CTRL` turns the skip into a failure.
#![cfg(unix)]

use aikit_cli::day_move_runner::{
    day_session, proposal_key, run_due, run_move, CentralError, CentralPort, CtrlCentral, DayAgent,
    Move, RunRequest, Trigger, TurnFailure, AGENT_WRITES_OFF, NO_PROVIDER, WRITE_HELD_AS_PROPOSE,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use tempfile::TempDir;

const SERVICE_TOKEN: &str = "disposable-day-agent-service-token-0123456789";
const HUMAN_TOKEN: &str = "disposable-day-agent-human-token-9876543210ab";
const SERVICE: &str = "native-service:aikit/day-agent";

// ---- the real ctrl -------------------------------------------------------------

fn has_action(ctrl: &Path, action: &str) -> bool {
    Command::new(ctrl)
        .args(["--json", "actions"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains(&format!("\"{action}\"")))
        .unwrap_or(false)
}

fn native_ctrl() -> Option<PathBuf> {
    let candidate = std::env::var_os("AIKIT_TEST_CTRL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ctrl"));
    let usable = has_action(&candidate, "central.day.moves.read");
    if !usable {
        assert!(
            std::env::var_os("AIKIT_REQUIRE_NATIVE_CTRL").is_none(),
            "AIKIT_REQUIRE_NATIVE_CTRL is set but no native `ctrl` with the Day's moves answers ({})",
            candidate.display()
        );
        eprintln!(
            "skip: no native `ctrl` with central.day.moves.read ({}); the Day's moves need Central's own Actions",
            candidate.display()
        );
        return None;
    }
    Some(candidate)
}

// ---- a disposable Central root with a Day -----------------------------------------

struct World {
    _dir: TempDir,
    root: PathBuf,
    ctrl: PathBuf,
}

fn digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

const DIE_FIELDS: [&str; 17] = [
    "p0_quick_thoughts",
    "p0_adjacencies",
    "p1_intentions",
    "p1_must",
    "p1_should",
    "p1_could",
    "p2_operations",
    "p2_manual_activity",
    "p3_patterns_noticed",
    "p3_recurring_themes",
    "p3_connections_made",
    "p4_files_touched",
    "p4_people_concepts",
    "p4_graph_additions",
    "p5_synthesis",
    "p5_quintessence",
    "p5_teleological_aim",
];

/// The page template's `ql-doc` (ql-daily-die.html), with the face 0 text and
/// face 5's proposal for tomorrow that a test gives it.
fn die_payload(ground: &str, tomorrow: &str) -> Value {
    let mut fields = serde_json::Map::new();
    for key in DIE_FIELDS {
        fields.insert(
            key.into(),
            if matches!(key, "p1_must" | "p1_should" | "p1_could") {
                json!([])
            } else {
                json!("")
            },
        );
    }
    fields.insert("p0_quick_thoughts".into(), json!(ground));
    json!({"meta": {"template": "ql-daily v0.1"}, "fields": fields,
        "body": {"p5_tomorrow": tomorrow, "p5_blockers": "", "p4_moments": []},
        "capture": [], "sessions": [
            {"session": "Pi on the harbour map", "time": "09:30", "commit": "", "files": "map.rs"}],
        "completed": [], "notes": [], "contributions": []})
}

impl World {
    fn new(ctrl: &Path, day_boundary_minutes: u32) -> World {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("Central");
        std::fs::create_dir_all(&root).unwrap();
        let init = Command::new(ctrl)
            .arg("--root")
            .arg(&root)
            .arg("init")
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );
        let world = World {
            _dir: dir,
            root,
            ctrl: ctrl.to_owned(),
        };
        std::fs::create_dir_all(world.root.join("Work/Alpha")).unwrap();
        let mut relations = Vec::new();
        for (name, role, value) in [
            (
                "placement.json",
                "work-placement-policy",
                json!({"schema":"central.work-placement-policy/v1","scope_ref":"control:root",
                       "writable":[{"path":"Work/Alpha","class":"repository"}],
                       "enforcement":"native-actions","required_coverage":["file-content"],"lease_seconds":3600}),
            ),
            (
                "civil-time-policy.json",
                "civil-time-policy",
                world.time_policy(day_boundary_minutes),
            ),
            (
                "native-action-authority.json",
                "native-action-authority",
                json!({"schema":"central.native-action-authority/v1","scope_ref":"control:root","grants":[
                    {"principal_ref":SERVICE,"actor_kind":"native-service","token_sha256":digest(SERVICE_TOKEN),
                     "scope_refs":["control:root"],
                     "actions":["central.day.move.record","central.receiving.submit","central.day.close-carry"],
                     "expires_at_unix_seconds":4102444800u64},
                    {"principal_ref":"human:disposable-test","actor_kind":"human","token_sha256":digest(HUMAN_TOKEN),
                     "scope_refs":["control:root"],
                     "actions":["central.day.ensure","central.day.settings.write","central.document.create",
                                "central.document.mutate","central.receiving.review","central.receiving.include"],
                     "expires_at_unix_seconds":4102444800u64}
                ]}),
            ),
        ] {
            let path = format!("Control/user/{name}");
            std::fs::write(
                world.root.join(&path),
                serde_json::to_vec_pretty(&value).unwrap(),
            )
            .unwrap();
            relations.push(json!({"ref":format!("central:source:control:root:{path}"),"path":path,
                "roles":[role],"provenance":"human-adopted","standing":"architecture-contract",
                "treatment":"control-user","recognition":"disposable-test-only","recorded_at_unix_seconds":1}));
        }
        std::fs::create_dir_all(world.root.join("Control/relations")).unwrap();
        std::fs::write(
            world.root.join("Control/relations/source-relations.json"),
            serde_json::to_vec_pretty(&json!({"schema":"central.control.ground-relations/v1",
                "project_id":"control:root","relations":relations}))
            .unwrap(),
        )
        .unwrap();
        world
    }

    fn time_policy(&self, day_boundary_minutes: u32) -> Value {
        json!({"schema":"central.civil-time-policy/v1","scope_ref":"control:root",
               "timezone":"Europe/London","day_boundary_minutes":day_boundary_minutes,
               "automatic_day_rollover":false})
    }

    /// Move the civil clock's Day boundary: the civil Day of "now" changes.
    fn set_day_boundary(&self, minutes: u32) {
        std::fs::write(
            self.root.join("Control/user/civil-time-policy.json"),
            serde_json::to_vec_pretty(&self.time_policy(minutes)).unwrap(),
        )
        .unwrap();
    }

    fn run(&self, action: &str, input: Value, token: Option<&str>) -> Value {
        let mut command = Command::new(&self.ctrl);
        command
            .args(["--json", "--root"])
            .arg(&self.root)
            .args(["action", "run", action, &input.to_string()])
            .env_remove("CENTRAL_NATIVE_TOKEN");
        if let Some(token) = token {
            command.env("CENTRAL_NATIVE_TOKEN", token);
        }
        let output = command.output().unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            panic!(
                "{action}: not a native envelope: {} / {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
    }

    fn read(&self, action: &str, input: Value) -> Value {
        let envelope = self.run(action, input, None);
        assert_eq!(envelope["ok"], true, "{action}: {envelope}");
        envelope["data"].clone()
    }

    fn human(&self, action: &str, input: Value) -> Value {
        let envelope = self.run(action, input, Some(HUMAN_TOKEN));
        assert_eq!(envelope["ok"], true, "{action}: {envelope}");
        envelope["data"].clone()
    }

    fn service(&self, action: &str, input: Value) -> Value {
        let envelope = self.run(action, input, Some(SERVICE_TOKEN));
        assert_eq!(envelope["ok"], true, "{action}: {envelope}");
        envelope["data"].clone()
    }

    fn central(&self) -> CtrlCentral {
        CtrlCentral {
            ctrl: self.ctrl.clone(),
            root: self.root.clone(),
            project: None,
            token: Some(SERVICE_TOKEN.into()),
        }
    }

    /// Open the civil Day of now (the person does).
    fn ensure_day(&self) -> String {
        let time = self.read("central.time.policy", json!({}));
        let day = self.human(
            "central.day.ensure",
            json!({"expected_time_policy_revision": time["revision"]}),
        );
        day["day_ref"].as_str().unwrap().to_owned()
    }

    /// Give a Day its document, mapped as the shell maps a Day
    /// (`desktop/cradle/kernel/src/central.rs`: one field per `fields` key).
    fn day_document(&self, day_ref: &str, id: &str, ground: &str, tomorrow: &str) -> Value {
        let day = self.read("central.day.read", json!({"day_ref": day_ref}));
        let policy = self.read("central.work.policy", json!({}));
        let payload = die_payload(ground, tomorrow);
        let fields: Vec<Value> = DIE_FIELDS
            .iter()
            .map(|key| json!({"id": key, "label": key, "template_pointer": format!("/fields/{key}")}))
            .collect();
        self.human(
            "central.document.create",
            json!({"kind":"day","document_id":id,"day_ref":day_ref,
                   "expected_revision":day["revision"]["revision"],
                   "expected_policy_revision":policy["revision"],
                   "template_payload":payload,"fields":fields}),
        )
    }

    fn save_settings(&self, patch: Value) -> Value {
        let current = self.read("central.day.settings.read", json!({}));
        let mut input = patch;
        input["expected_revision"] = current["revision"].clone();
        self.human("central.day.settings.write", input)
    }

    fn day(&self, day_ref: &str) -> Value {
        self.read("central.day.read", json!({"day_ref": day_ref}))
    }

    fn moves(&self, day_ref: &str) -> Value {
        self.read("central.day.moves.read", json!({"day_ref": day_ref}))
    }

    /// Every Return in the receiving ledger, read in full.
    fn returns(&self) -> Vec<Value> {
        let page = self.read("central.receiving.list", json!({"limit": 200}));
        page["returns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                self.read(
                    "central.receiving.read",
                    json!({"return_ref": row["return_ref"]}),
                )["record"]
                    .clone()
            })
            .collect()
    }
}

// ---- the Day's agent, scripted --------------------------------------------------------

/// A Day's agent that answers from a script and remembers what it was asked.
struct Scripted {
    answers: Mutex<Vec<std::result::Result<String, TurnFailure>>>,
    prompts: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(answers: Vec<std::result::Result<String, TurnFailure>>) -> Scripted {
        Scripted {
            answers: Mutex::new(answers),
            prompts: Mutex::new(vec![]),
        }
    }
    fn says(answer: &str) -> Scripted {
        Scripted::new(vec![Ok(answer.to_owned())])
    }
    fn silent() -> Scripted {
        Scripted::new(vec![])
    }
    fn asked(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

impl DayAgent for Scripted {
    fn agent_ref(&self, _: &str) -> String {
        "agent:the-day:test".into()
    }
    fn session_ref(&self, _: &str) -> Option<String> {
        Some("agent-session/scripted".into())
    }
    fn turn(&self, _: &str, prompt: &str) -> std::result::Result<String, TurnFailure> {
        self.prompts.lock().unwrap().push(prompt.to_owned());
        let mut answers = self.answers.lock().unwrap();
        if answers.is_empty() {
            return Err(TurnFailure::Failed(
                "the scripted agent was asked again".into(),
            ));
        }
        answers.remove(0)
    }
}

fn press(mv: Move, day_ref: &str) -> RunRequest {
    RunRequest {
        mv,
        day_ref: Some(day_ref.to_owned()),
        trigger: Trigger::Press,
        occurrence_ref: None,
    }
}

const TASKS: &str =
    r#"{"must":["Ring the printer","Send the invoice"],"should":["Water the plants"],"could":[]}"#;

// ---- Off ---------------------------------------------------------------------------------

#[test]
fn agent_writes_off_records_a_refused_run_and_submits_nothing() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:off", "Ring the printer.", "");
    world.save_settings(json!({"agent_writes": "off"}));

    let agent = Scripted::silent();
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();

    assert_eq!(report["state"], "refused", "{report}");
    assert_eq!(report["reason"], AGENT_WRITES_OFF);
    assert_eq!(report["agent_writes"], "off");
    assert!(agent.asked().is_empty(), "Off takes no turn");
    assert!(world.returns().is_empty(), "Off submits nothing");
    // The record shows it: the run began under Off and ended refused, with the why.
    let moves = world.moves(&day_ref);
    let last = &moves["moves"]["tasks"]["last"];
    assert_eq!(last["state"], "refused");
    assert_eq!(last["agent_writes"], "off");
    assert_eq!(last["end"]["reason"], AGENT_WRITES_OFF);
    assert_eq!(moves["moves"]["tasks"]["running"], Value::Null);
}

// ---- Propose -----------------------------------------------------------------------------

#[test]
fn propose_submits_one_proposal_per_entry_each_on_the_field_it_belongs_to() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(
        &day_ref,
        "day:propose",
        "<p>Ring the printer. The invoice is late.</p>",
        "",
    );
    // The person made the move's prompt their own: the run uses Central's, not AIKit's.
    world.save_settings(json!({"moves": {"tasks": {"prompt": "Be brief and concrete."}}}));

    let agent = Scripted::says(TASKS);
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();

    assert_eq!(report["state"], "done", "{report}");
    assert_eq!(report["outcome"], "ran");
    let gave = &report["gave"];
    let proposals = gave["proposals"].as_array().unwrap();
    assert_eq!(proposals.len(), 3, "three entries, three proposals: {gave}");
    assert_eq!(gave["counts"]["must"], 2);
    assert_eq!(gave["counts"]["should"], 1);
    assert_eq!(gave["counts"]["proposed"], 3);
    assert!(gave["summary"].as_str().unwrap().contains("3 proposals"));
    assert!(gave.get("writes").is_none() || gave["writes"].is_null());

    // What the agent was asked: Central's prompt, then face 0's text, then the shape.
    let prompts = agent.asked();
    assert_eq!(prompts.len(), 1, "one turn");
    assert!(
        prompts[0].starts_with("Be brief and concrete."),
        "{}",
        prompts[0]
    );
    assert!(prompts[0].contains("Ring the printer. The invoice is late."));
    assert!(prompts[0].contains("Face 0, Ground"));
    assert!(prompts[0].contains(r#""must""#) && prompts[0].contains("nothing else"));
    assert!(!prompts[0].contains("Face 5"), "Tasks reads face 0 only");

    // Each proposal is a field.append on the right field, authored by the Day's agent.
    let returns = world.returns();
    assert_eq!(returns.len(), 3);
    let mut landed: Vec<(String, String)> = returns
        .iter()
        .map(|record| {
            assert_eq!(record["status"], "pending", "a proposal waits: {record}");
            assert_eq!(record["proposal"]["operation"], "field.append");
            assert_eq!(record["declared_producer"]["ref"], "agent:the-day:test");
            assert_eq!(record["declared_producer"]["actor_kind"], "agent");
            assert_eq!(record["author"]["principal_ref"], SERVICE);
            assert_eq!(record["day_ref"], day_ref.as_str());
            assert_eq!(record["run_ref"], report["run_ref"]);
            assert_eq!(record["session_ref"], "agent-session/scripted");
            let id = record["proposal"]["contribution_id"].as_str().unwrap();
            assert!(id.starts_with(report["run_ref"].as_str().unwrap()), "{id}");
            (
                record["proposal"]["field_id"].as_str().unwrap().to_owned(),
                record["proposal"]["html"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    landed.sort();
    assert_eq!(
        landed,
        vec![
            ("p1_must".to_owned(), "<p>Ring the printer</p>".to_owned()),
            ("p1_must".to_owned(), "<p>Send the invoice</p>".to_owned()),
            ("p1_should".to_owned(), "<p>Water the plants</p>".to_owned()),
        ]
    );
    // A proposal is not a write: the Day's page holds none of it yet.
    let day = world.day(&day_ref);
    assert_eq!(day["document"]["document"]["contributions"], json!([]));
    // The run is on Central's record, with what it gave.
    let moves = world.moves(&day_ref);
    let last = &moves["moves"]["tasks"]["last"];
    assert_eq!(last["state"], "done");
    assert_eq!(last["prompt"], "Be brief and concrete.");
    assert_eq!(
        last["end"]["gave"]["proposals"].as_array().unwrap().len(),
        3
    );
}

// ---- Write ---------------------------------------------------------------------------------

#[test]
fn write_is_held_as_propose_and_the_run_says_so() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:write", "Plan the week.", "");
    world.save_settings(json!({"agent_writes": "write"}));

    let agent = Scripted::says(TASKS);
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();

    assert_eq!(report["state"], "done", "{report}");
    assert_eq!(report["agent_writes"], "write");
    let gave = &report["gave"];
    assert!(
        gave["summary"]
            .as_str()
            .unwrap()
            .contains(WRITE_HELD_AS_PROPOSE),
        "{gave}"
    );
    assert!(
        WRITE_HELD_AS_PROPOSE.contains("Central admits no direct agent write on a Day"),
        "the sentence names the reason"
    );
    assert_eq!(gave["proposals"].as_array().unwrap().len(), 3);
    assert!(
        gave.get("writes").is_none() || gave["writes"].is_null(),
        "nothing was written"
    );
    assert_eq!(world.returns().len(), 3);
    assert_eq!(
        world.day(&day_ref)["document"]["document"]["contributions"],
        json!([])
    );
}

// ---- an answer that does not parse ------------------------------------------------------------

#[test]
fn an_answer_that_does_not_parse_stalls_the_run_and_submits_nothing() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:prose", "Plan the week.", "");

    let agent = Scripted::says("Sure! Here are some tasks I would suggest: ring the printer.");
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();

    assert_eq!(report["state"], "stalled", "{report}");
    let reason = report["reason"].as_str().unwrap();
    assert!(reason.contains("did not parse"), "{reason}");
    assert!(reason.contains("Tasks"), "{reason}");
    assert!(world.returns().is_empty());
    // A begun run never stays running.
    let moves = world.moves(&day_ref);
    assert_eq!(moves["moves"]["tasks"]["running"], Value::Null);
    assert_eq!(moves["moves"]["tasks"]["last"]["state"], "stalled");

    // The same for a well-formed document of the wrong shape.
    let agent = Scripted::says(r#"{"must":["one"],"should":[]}"#);
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();
    assert_eq!(report["state"], "stalled", "{report}");
    assert!(world.returns().is_empty());
}

#[test]
fn a_turn_that_gives_no_answer_stalls_and_no_provider_refuses_in_aikits_words() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:turns", "Plan the week.", "");

    let agent = Scripted::new(vec![Err(TurnFailure::Failed("the harness hung up".into()))]);
    let report = run_move(&world.central(), &agent, &press(Move::Patterns, &day_ref)).unwrap();
    assert_eq!(report["state"], "stalled", "{report}");
    assert_eq!(report["reason"], "the harness hung up");

    let agent = Scripted::new(vec![Err(TurnFailure::Unconfigured(NO_PROVIDER.into()))]);
    let report = run_move(&world.central(), &agent, &press(Move::Patterns, &day_ref)).unwrap();
    assert_eq!(report["state"], "refused", "{report}");
    assert!(report["reason"]
        .as_str()
        .unwrap()
        .starts_with("no encounter provider is configured"));
    assert_eq!(
        world.moves(&day_ref)["moves"]["patterns"]["running"],
        Value::Null
    );
}

// ---- a Day with no document ----------------------------------------------------------------------

#[test]
fn a_day_without_a_document_refuses_the_run_and_takes_no_turn() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();

    let agent = Scripted::silent();
    let report = run_move(&world.central(), &agent, &press(Move::Tasks, &day_ref)).unwrap();
    assert_eq!(report["state"], "refused", "{report}");
    assert!(report["reason"]
        .as_str()
        .unwrap()
        .contains("no Day document"));
    assert!(agent.asked().is_empty());
}

// ---- a retry of the same run ------------------------------------------------------------------------

/// Central as a runner killed mid-run sees it: the second proposal never arrives
/// and the run's end is never recorded.
struct Dying<'a> {
    inner: &'a CtrlCentral,
    fail_second_submit: AtomicBool,
    fail_end: AtomicBool,
    submits: AtomicUsize,
}

impl CentralPort for Dying<'_> {
    fn call(&self, action: &str, input: &Value) -> Result<Value, CentralError> {
        let dead = |what: &str| CentralError {
            code: "day_move.central_unavailable".into(),
            message: format!("injected: the runner died before {what}"),
            unavailable: true,
        };
        if action == "central.receiving.submit" {
            let n = self.submits.fetch_add(1, Ordering::SeqCst);
            if n == 1 && self.fail_second_submit.load(Ordering::SeqCst) {
                return Err(dead("the second proposal"));
            }
        }
        if action == "central.day.move.record"
            && input["event"] == "end"
            && self.fail_end.load(Ordering::SeqCst)
        {
            return Err(dead("recording the end"));
        }
        self.inner.call(action, input)
    }
}

#[test]
fn a_retry_of_the_same_run_does_not_double_submit() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:retry", "Plan the week.", "");
    // A saved schedule gives the run an occurrence, so the same run can be asked again.
    world.save_settings(
        json!({"moves": {"tasks": {"runs_at": {"kind": "daily", "time": "00:00"}}}}),
    );
    let due = world.read(
        "central.day.moves.due",
        json!({"since_unix_ms": now_ms() - 3 * 24 * 3600 * 1000, "now_unix_ms": now_ms()}),
    );
    let occurrence = due["due"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["day_ref"] == day_ref.as_str())
        .expect("today's occurrence")["occurrence_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    let request = RunRequest {
        mv: Move::Tasks,
        day_ref: Some(day_ref.clone()),
        trigger: Trigger::Schedule,
        occurrence_ref: Some(occurrence.clone()),
    };

    // First attempt: one proposal reaches Central, then the runner dies.
    let central = world.central();
    let dying = Dying {
        inner: &central,
        fail_second_submit: AtomicBool::new(true),
        fail_end: AtomicBool::new(true),
        submits: AtomicUsize::new(0),
    };
    let first = Scripted::says(
        r#"{"must":["Ring the printer","Send the invoice"],"should":[],"could":[]}"#,
    );
    assert!(
        run_move(&dying, &first, &request).is_err(),
        "the end could not be recorded"
    );
    assert_eq!(
        world.returns().len(),
        1,
        "one proposal arrived before the death"
    );
    assert_eq!(
        world.moves(&day_ref)["moves"]["tasks"]["running"]["occurrence_ref"],
        occurrence.as_str(),
        "the dead runner left the run open"
    );

    // While the runner was dead the person wrote on the Day: its document moved on,
    // so the retry proposes against a newer revision than the proposal that arrived.
    let day = world.day(&day_ref);
    world.human(
        "central.document.mutate",
        json!({"operation":"field.set","field_id":"p0_adjacencies","value":"<p>Also the stapler.</p>",
               "source_ref":day["document"]["source"]["ref"],"document_id":"day:retry",
               "expected_revision":day["document"]["revision"]["revision"],"request_id":"person-edit-1"}),
    );

    // The retry of the same run: it completes it. The proposal that arrived is not
    // submitted twice; the one that did not, is.
    let second = Scripted::says(
        r#"{"must":["Ring the printer","Send the invoice"],"should":[],"could":[]}"#,
    );
    let report = run_move(&central, &second, &request).unwrap();
    assert!(
        second.asked()[0].contains("Also the stapler."),
        "the retry reads the Day as it stands now"
    );
    assert_eq!(report["outcome"], "resumed", "{report}");
    assert_eq!(report["state"], "done", "{report}");
    assert_eq!(report["gave"]["proposals"].as_array().unwrap().len(), 2);
    let returns = world.returns();
    assert_eq!(returns.len(), 2, "two entries, two proposals, never three");
    let mut ids: Vec<&str> = returns
        .iter()
        .map(|record| record["proposal"]["contribution_id"].as_str().unwrap())
        .collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 2);
    let run_ref = report["run_ref"].as_str().unwrap();
    assert!(ids.contains(&proposal_key(run_ref, "p1_must", "<p>Ring the printer</p>").as_str()));

    // Asked a third time, the run has ended: it is not run again.
    let third = Scripted::silent();
    let report = run_move(&central, &third, &request).unwrap();
    assert_eq!(report["outcome"], "skipped", "{report}");
    assert!(third.asked().is_empty());
    assert_eq!(world.returns().len(), 2);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ---- due ---------------------------------------------------------------------------------------------------

#[test]
fn due_runs_a_not_begun_occurrence_once_and_skips_a_begun_one() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(&day_ref, "day:due", "Plan the week.", "");
    // On the defaults nothing runs by itself: due says so and runs nothing.
    let silent = Scripted::silent();
    let nothing = run_due(&world.central(), &silent, now_ms() - 3_600_000, now_ms()).unwrap();
    assert_eq!(nothing["outcomes"], json!([]));
    assert!(nothing["reason"]
        .as_str()
        .unwrap()
        .contains("nothing runs by itself"));

    world.save_settings(json!({"moves": {
        "tasks": {"runs_at": {"kind": "daily", "time": "00:00"}},
        "patterns": {"runs_at": {"kind": "daily", "time": "00:00"}}}}));
    let wide = world.read(
        "central.day.moves.due",
        json!({"since_unix_ms": now_ms() - 3 * 24 * 3600 * 1000, "now_unix_ms": now_ms()}),
    );
    let todays: Vec<&Value> = wide["due"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["day_ref"] == day_ref.as_str())
        .collect();
    assert_eq!(todays.len(), 2, "tasks and patterns, today: {wide}");
    let at = todays[0]["due_unix_ms"].as_i64().unwrap();
    // The patterns occurrence began already (a runner took it and is still on it).
    let patterns = todays.iter().find(|row| row["move"] == "patterns").unwrap()["occurrence_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    world.service(
        "central.day.move.record",
        json!({"event":"begin","move":"patterns","at_unix_ms":now_ms(),"trigger":"schedule",
               "occurrence_ref":patterns,"day_ref":day_ref}),
    );

    let agent = Scripted::says(TASKS);
    let ran = run_due(&world.central(), &agent, at - 1, at).unwrap();
    let outcomes = ran["outcomes"].as_array().unwrap();
    assert_eq!(outcomes.len(), 2, "{ran}");
    let tasks = outcomes.iter().find(|o| o["move"] == "tasks").unwrap();
    assert_eq!(tasks["outcome"], "ran", "{ran}");
    assert_eq!(tasks["state"], "done");
    assert_eq!(tasks["trigger"], "schedule");
    assert_eq!(
        tasks["occurrence_ref"],
        todays.iter().find(|r| r["move"] == "tasks").unwrap()["occurrence_ref"]
    );
    let skipped = outcomes.iter().find(|o| o["move"] == "patterns").unwrap();
    assert_eq!(skipped["outcome"], "skipped", "{ran}");
    assert_eq!(
        agent.asked().len(),
        1,
        "one turn, for the one occurrence that had not begun"
    );
    assert_eq!(world.returns().len(), 3);

    // Asked again, nothing is left to run.
    let again = run_due(&world.central(), &agent, at - 1, at).unwrap();
    assert!(again["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|outcome| outcome["outcome"] == "skipped"));
    assert_eq!(agent.asked().len(), 1);
    assert_eq!(world.returns().len(), 3);
}

// ---- Close --------------------------------------------------------------------------------------------------------

const CLOSE: &str = r#"{"summary":"The harbour map took the day.","learned":["Maps need a legend","Ask before drawing"]}"#;

#[test]
fn close_without_the_persons_saved_consent_proposes_the_summary_and_does_not_close() {
    let Some(ctrl) = native_ctrl() else { return };
    let world = World::new(&ctrl, 0);
    let day_ref = world.ensure_day();
    world.day_document(
        &day_ref,
        "day:close-denied",
        "Plan the week.",
        "<p>Begin with the harbour map.</p>",
    );

    let agent = Scripted::says(CLOSE);
    let report = run_move(&world.central(), &agent, &press(Move::Close, &day_ref)).unwrap();

    assert_eq!(report["state"], "stalled", "{report}");
    let reason = report["reason"].as_str().unwrap();
    assert!(reason.contains("Central did not close the Day"), "{reason}");
    assert!(reason.contains("saved"), "Central's own words: {reason}");
    assert!(report["gave"]["summary"]
        .as_str()
        .unwrap()
        .contains("The Day was not closed"));
    assert_eq!(report["gave"]["proposals"].as_array().unwrap().len(), 1);
    assert_eq!(world.day(&day_ref)["temporal"]["lifecycle"], "open");
}

#[test]
fn close_calls_close_carry_and_gives_its_state_and_the_carry_proposal() {
    let Some(ctrl) = native_ctrl() else { return };
    // The Day being closed must be a Day the civil clock has left. Open it with the
    // Day boundary at 23:59, so "now" is still yesterday; then move the boundary back
    // to midnight, so now is the next civil Day.
    let world = World::new(&ctrl, 1439);
    let closing = world.ensure_day();
    world.day_document(
        &closing,
        "day:closing",
        "Plan the week.",
        "<p>Begin with the harbour map.</p>",
    );
    world.set_day_boundary(0);
    let tomorrow = world.ensure_day();
    assert_ne!(tomorrow, closing, "the civil Day moved on");
    world.day_document(&tomorrow, "day:tomorrow", "", "");
    // The person saved Close at Day start: that is the consent an agent closes under.
    world.save_settings(json!({"moves": {"close": {"runs_at": {"kind": "day_start"}}}}));

    let agent = Scripted::says(CLOSE);
    let report = run_move(&world.central(), &agent, &press(Move::Close, &closing)).unwrap();

    assert_eq!(report["state"], "done", "{report}");
    let gave = &report["gave"];
    assert_eq!(gave["counts"]["carry_state"], "waits", "{gave}");
    assert_eq!(gave["counts"]["day_closed"], true);
    assert_eq!(gave["counts"]["learned"], 2);
    let summary = gave["summary"].as_str().unwrap();
    assert!(summary.contains("The Day is closed"), "{summary}");
    assert!(
        summary.contains("no knowledge return was made"),
        "{summary}"
    );
    // The summary on face 5 and the carry for tomorrow's face 0: two proposals.
    assert_eq!(gave["proposals"].as_array().unwrap().len(), 2, "{gave}");
    assert_eq!(world.day(&closing)["temporal"]["lifecycle"], "closed");
    let returns = world.returns();
    assert_eq!(returns.len(), 2);
    let carry = returns
        .iter()
        .find(|record| record["proposal"]["field_id"] == "p0_quick_thoughts")
        .expect("the carry proposal");
    assert!(carry["proposal"]["html"]
        .as_str()
        .unwrap()
        .contains("harbour map"));
    assert_eq!(carry["document_id"], "day:tomorrow");
    assert_eq!(carry["declared_producer"]["ref"], "agent:the-day:test");
    let summary_proposal = returns
        .iter()
        .find(|record| record["proposal"]["field_id"] == "p5_synthesis")
        .expect("the summary proposal");
    assert!(summary_proposal["proposal"]["html"]
        .as_str()
        .unwrap()
        .contains("harbour map took the day"));
    assert_eq!(summary_proposal["document_id"], "day:closing");
    // The agent was asked about face 5 only.
    let prompts = agent.asked();
    assert!(prompts[0].contains("Face 5, Synthesis") && !prompts[0].contains("Face 0, Ground"));
    // Tomorrow's page holds nothing until the person accepts the carry.
    assert_eq!(
        world.day(&tomorrow)["document"]["document"]["contributions"],
        json!([])
    );
    // Central's record has the run, ended done, with the carry state in what it gave.
    let moves = world.moves(&closing);
    assert_eq!(moves["moves"]["close"]["last"]["state"], "done");
    assert_eq!(
        moves["moves"]["close"]["last"]["end"]["gave"]["counts"]["carry_state"],
        "waits"
    );
    // A closed Day takes no further run.
    let again = run_move(
        &world.central(),
        &Scripted::silent(),
        &press(Move::Close, &closing),
    );
    assert!(
        again.is_err(),
        "a closed Day is kept and no move begins on it"
    );
}

// ---- the command line ----------------------------------------------------------------------------------------------------

#[test]
fn a_scheduled_run_names_its_occurrence_and_a_press_has_none() {
    // Refused before Central is asked anything: `ctrl` is never run.
    let central = CtrlCentral {
        ctrl: PathBuf::from("/nonexistent/ctrl"),
        root: PathBuf::from("/nonexistent/Central"),
        project: None,
        token: None,
    };
    let agent = Scripted::silent();
    let mut request = press(Move::Tasks, "central:day:control:root:2026-10-20");
    request.occurrence_ref = Some("tasks:daily:1".into());
    let error = run_move(&central, &agent, &request).unwrap_err();
    assert_eq!(error.code(), "day_move.occurrence_unexpected");
    request.occurrence_ref = None;
    request.trigger = Trigger::Schedule;
    let error = run_move(&central, &agent, &request).unwrap_err();
    assert_eq!(error.code(), "day_move.occurrence_required");
    assert!(agent.asked().is_empty());
}

mod cli {
    use super::*;
    use aikit_cli::encounter_service::{
        request, socket_path, EncounterProvider, EncounterRequest, EncounterService,
    };
    use aikit_store::{AikitHome, SessionSpaceApplicationStore};

    /// Stops the resident owner these tests started, and only it: the owner of
    /// the temporary home, by the pid it answered.
    struct OwnerGuard(AikitHome);
    impl Drop for OwnerGuard {
        fn drop(&mut self) {
            let socket = socket_path(&self.0);
            if let Ok(health) = request(&socket, &EncounterRequest::Health) {
                if let Some(pid) = health["data"]["pid"].as_u64() {
                    let _ = request(
                        &socket,
                        &EncounterRequest::Shutdown {
                            expected_pid: pid as u32,
                        },
                    );
                }
            }
        }
    }

    /// AIKit's controlled protocol peer, answering a Day move with a canned
    /// answer per move (not a model), and keeping every prompt it was given.
    const PEER: &str = r#"
import json, os, re, sys, time
log, answers = sys.argv[1], json.load(open(sys.argv[2]))
native = "day-native"
news = 0
waiting = None
def emit(value):
    os.write(1, (json.dumps(value) + "\n").encode())
def result(ident, value):
    emit({"jsonrpc": "2.0", "id": ident, "result": value})
def config():
    return [{"id": "model", "category": "model", "type": "select", "name": "Model", "currentValue": "test/a",
             "options": [{"value": "test/a", "name": "A"}]}]
def modes():
    return {"currentModeId": "default", "availableModes": [{"id": "default", "name": "Default"}]}
for line in sys.stdin:
    message = json.loads(line)
    method, ident = message.get("method"), message.get("id")
    if method == "initialize":
        result(ident, {"protocolVersion": 1, "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"resume": True}}})
    elif method in ("session/new", "session/load", "session/resume"):
        news += method == "session/new"
        native = message.get("params", {}).get("sessionId", native)
        result(ident, {"sessionId": native, "configOptions": config(), "modes": modes()})
    elif method == "session/prompt":
        prompt = "".join(item.get("text", "") for item in message["params"]["prompt"])
        with open(log, "a") as out:
            out.write(json.dumps({"pid": os.getpid(), "news": news, "at": time.time(), "prompt": prompt}) + "\n")
        found = re.findall(r"the (\w+) move\.", prompt)
        answer = answers.get(found[-1].lower() if found else "", "I have no answer for that.")
        if answer.startswith("__slow__"):
            time.sleep(2)
            answer = answer[len("__slow__"):]
        if answer == "__silence__":
            waiting = ident
            continue
        if answer == "__permission__":
            waiting = ident
            emit({"jsonrpc": "2.0", "id": "permission-1", "method": "session/request_permission", "params": {
                "sessionId": native, "toolCall": {"toolCallId": "t1", "title": "Run a tool", "kind": "execute", "status": "pending"},
                "options": [{"optionId": "deny", "name": "Deny", "kind": "reject_once"}, {"optionId": "allow", "name": "Allow", "kind": "allow_once"}]}})
            continue
        emit({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": native,
              "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": answer}}}})
        result(ident, {"stopReason": "end_turn"})
    elif method == "session/cancel" and waiting is not None:
        result(waiting, {"stopReason": "cancelled"})
        waiting = None
    elif ident == "permission-1" and waiting is not None:
        result(waiting, {"stopReason": "cancelled"})
        waiting = None
"#;

    struct Rig {
        world: World,
        home: AikitHome,
        _home_dir: TempDir,
        log: PathBuf,
        _owner: OwnerGuard,
    }

    impl Rig {
        fn new(ctrl: &Path, configure_provider: bool) -> Rig {
            Rig::build(
                ctrl,
                configure_provider.then(|| {
                    json!({"tasks": TASKS,
                           "patterns": r#"[{"pattern":"Reading before writing","items":["Pi on the harbour map"]}]"#})
                }),
            )
        }

        /// A rig whose one configured provider answers each move as `answers` says.
        fn build(ctrl: &Path, answers: Option<Value>) -> Rig {
            let world = World::new(ctrl, 0);
            let home_dir = TempDir::new().unwrap();
            let home = AikitHome::at(home_dir.path().join("aikit-home"));
            std::fs::create_dir_all(home.root()).unwrap();
            let log = home_dir.path().join("peer.log");
            if let Some(answers_json) = answers {
                let script = home_dir.path().join("peer.py");
                std::fs::write(&script, PEER).unwrap();
                let answers = home_dir.path().join("answers.json");
                std::fs::write(&answers, answers_json.to_string()).unwrap();
                EncounterService::configure(
                    &home,
                    EncounterProvider {
                        protocol: Default::default(),
                        id: "day-peer".into(),
                        label: "Controlled protocol peer (not a model)".into(),
                        argv: vec![
                            "python3".into(),
                            "-u".into(),
                            script.display().to_string(),
                            log.display().to_string(),
                            answers.display().to_string(),
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
            }
            Rig {
                world,
                _owner: OwnerGuard(home.clone()),
                home,
                _home_dir: home_dir,
                log,
            }
        }

        fn aikit(&self, args: &[&str]) -> (bool, Value) {
            let output = Command::new(env!("CARGO_BIN_EXE_aikit"))
                .args(args)
                .arg("--central-root")
                .arg(&self.world.root)
                .arg("--ctrl-bin")
                .arg(&self.world.ctrl)
                .arg("--json")
                .env("AIKIT_HOME", self.home.root())
                .env("HOME", self.home.root())
                .env("CENTRAL_NATIVE_TOKEN", SERVICE_TOKEN)
                .env_remove("AIKIT_CONTEXT_ID")
                .env_remove("AIKIT_ISOLATION")
                .env_remove("AIKIT_DAY_PROVIDER")
                .current_dir(self.home.root())
                .output()
                .unwrap();
            let envelope: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
                panic!(
                    "aikit {args:?}: {} / {}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                )
            });
            (output.status.success(), envelope)
        }

        fn peer_prompts(&self) -> Vec<Value> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    #[test]
    fn with_no_provider_configured_the_run_is_refused_in_aikits_words() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::new(&ctrl, false);
        let day_ref = rig.world.ensure_day();
        rig.world
            .day_document(&day_ref, "day:no-provider", "Plan the week.", "");

        let (ok, envelope) = rig.aikit(&["work", "day-move", "run", "--move", "tasks"]);
        assert!(ok, "a recorded refusal is a recorded outcome: {envelope}");
        let data = &envelope["data"];
        assert_eq!(data["state"], "refused", "{envelope}");
        assert!(
            data["reason"]
                .as_str()
                .unwrap()
                .starts_with("no encounter provider is configured"),
            "{envelope}"
        );
        assert!(rig.world.returns().is_empty());
        assert_eq!(
            rig.world.moves(&day_ref)["moves"]["tasks"]["last"]["state"],
            "refused"
        );
    }

    #[test]
    fn a_move_takes_one_turn_in_the_days_one_persistent_session_and_proposes_what_it_gave() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::new(&ctrl, true);
        let day_ref = rig.world.ensure_day();
        rig.world.day_document(
            &day_ref,
            "day:cli",
            "<p>Ring the printer. The invoice is late.</p>",
            "",
        );

        let (ok, envelope) = rig.aikit(&["work", "day-move", "run", "--move", "tasks"]);
        assert!(ok, "{envelope}");
        let data = &envelope["data"];
        assert_eq!(data["state"], "done", "{envelope}");
        assert_eq!(data["gave"]["proposals"].as_array().unwrap().len(), 3);
        let prompts = rig.peer_prompts();
        assert_eq!(prompts.len(), 1);
        let first = prompts[0]["prompt"].as_str().unwrap();
        assert!(
            first.contains("Read face 0, the Ground of the Day"),
            "Central's own prompt: {first}"
        );
        assert!(
            first.contains("The invoice is late."),
            "face 0's text: {first}"
        );
        let returns = rig.world.returns();
        assert_eq!(returns.len(), 3);
        assert!(returns
            .iter()
            .all(|record| record["declared_producer"]["ref"] == "agent:the-day:control:root"));

        // A second move: the same persistent session, one SessionSpace labelled "the Day".
        let (ok, envelope) = rig.aikit(&["work", "day-move", "run", "--move", "patterns"]);
        assert!(ok, "{envelope}");
        assert_eq!(envelope["data"]["state"], "done", "{envelope}");
        let prompts = rig.peer_prompts();
        assert_eq!(prompts.len(), 2);
        assert_eq!(
            prompts[0]["pid"], prompts[1]["pid"],
            "one peer process for both moves"
        );
        assert_eq!(
            (prompts[0]["news"].as_u64(), prompts[1]["news"].as_u64()),
            (Some(1), Some(1)),
            "one native session opened for both moves"
        );
        let spaces = SessionSpaceApplicationStore::new(rig.home.clone())
            .list()
            .unwrap();
        let days: Vec<_> = spaces
            .iter()
            .filter(|space| space.label.as_deref() == Some("the Day"))
            .collect();
        assert_eq!(days.len(), 1, "one Day session for the World");
        let (space, session) = day_session("control:root");
        assert_eq!(days[0].definition.id, space);
        assert!(days[0].agent_sessions.contains_key(&session));
        assert_eq!(rig.world.returns().len(), 4);
    }

    #[test]
    fn due_through_the_command_line_runs_each_occurrence_once() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::new(&ctrl, true);
        let day_ref = rig.world.ensure_day();
        rig.world
            .day_document(&day_ref, "day:cli-due", "Plan the week.", "");
        rig.world.save_settings(
            json!({"moves": {"tasks": {"runs_at": {"kind": "daily", "time": "00:00"}}}}),
        );
        let wide = rig.world.read(
            "central.day.moves.due",
            json!({"since_unix_ms": now_ms() - 3 * 24 * 3600 * 1000, "now_unix_ms": now_ms()}),
        );
        let at = wide["due"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["day_ref"] == day_ref.as_str())
            .unwrap()["due_unix_ms"]
            .as_i64()
            .unwrap();
        let since = (at - 1).to_string();
        let now = at.to_string();

        let (ok, envelope) =
            rig.aikit(&["work", "day-move", "due", "--since", &since, "--now", &now]);
        assert!(ok, "{envelope}");
        let outcomes = envelope["data"]["outcomes"].as_array().unwrap();
        assert_eq!(outcomes.len(), 1, "{envelope}");
        assert_eq!(outcomes[0]["outcome"], "ran");
        assert_eq!(outcomes[0]["state"], "done");
        let (_, again) = rig.aikit(&["work", "day-move", "due", "--since", &since, "--now", &now]);
        assert_eq!(
            again["data"]["outcomes"][0]["outcome"], "skipped",
            "{again}"
        );
        assert_eq!(rig.peer_prompts().len(), 1);
        assert_eq!(rig.world.returns().len(), 3);
    }

    #[test]
    fn a_turn_that_asks_for_a_tools_permission_is_cancelled_and_the_run_stalls() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::build(&ctrl, Some(json!({"tasks": "__permission__"})));
        let day_ref = rig.world.ensure_day();
        rig.world
            .day_document(&day_ref, "day:permission", "Plan the week.", "");

        let (ok, envelope) = rig.aikit(&["work", "day-move", "run", "--move", "tasks"]);
        assert!(ok, "{envelope}");
        let data = &envelope["data"];
        assert_eq!(data["state"], "stalled", "{envelope}");
        assert!(
            data["reason"].as_str().unwrap().contains("permission"),
            "{envelope}"
        );
        assert!(rig.world.returns().is_empty());
        assert_eq!(
            rig.world.moves(&day_ref)["moves"]["tasks"]["running"],
            Value::Null,
            "a begun run never stays running"
        );
    }

    #[test]
    fn a_turn_that_never_ends_is_cancelled_at_the_timeout_and_the_run_stalls() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::build(&ctrl, Some(json!({"tasks": "__silence__"})));
        let day_ref = rig.world.ensure_day();
        rig.world
            .day_document(&day_ref, "day:silence", "Plan the week.", "");

        let (ok, envelope) = rig.aikit(&[
            "work",
            "day-move",
            "run",
            "--move",
            "tasks",
            "--timeout-secs",
            "2",
        ]);
        assert!(ok, "{envelope}");
        let data = &envelope["data"];
        assert_eq!(data["state"], "stalled", "{envelope}");
        assert!(
            data["reason"]
                .as_str()
                .unwrap()
                .contains("did not finish within 2 seconds"),
            "{envelope}"
        );
        assert!(rig.world.returns().is_empty());
    }

    #[test]
    fn two_moves_pressed_together_take_their_turns_one_after_the_other() {
        let Some(ctrl) = native_ctrl() else { return };
        let rig = Rig::build(
            &ctrl,
            Some(json!({"tasks": format!("__slow__{TASKS}"),
                        "patterns": r#"__slow__[{"pattern":"Reading before writing","items":["Pi on the harbour map"]}]"#})),
        );
        let day_ref = rig.world.ensure_day();
        rig.world
            .day_document(&day_ref, "day:together", "Plan the week.", "");

        let (tasks, patterns) = std::thread::scope(|scope| {
            let tasks = scope.spawn(|| rig.aikit(&["work", "day-move", "run", "--move", "tasks"]));
            let patterns =
                scope.spawn(|| rig.aikit(&["work", "day-move", "run", "--move", "patterns"]));
            (tasks.join().unwrap(), patterns.join().unwrap())
        });
        assert!(tasks.0 && patterns.0, "{} / {}", tasks.1, patterns.1);
        assert_eq!(tasks.1["data"]["state"], "done", "{}", tasks.1);
        assert_eq!(patterns.1["data"]["state"], "done", "{}", patterns.1);
        let prompts = rig.peer_prompts();
        assert_eq!(prompts.len(), 2);
        let (first, second) = (
            prompts[0]["at"].as_f64().unwrap(),
            prompts[1]["at"].as_f64().unwrap(),
        );
        assert!(
            second - first >= 1.8,
            "the second move's turn began {:.1}s after the first's, which takes 2s",
            second - first
        );
        assert_eq!(
            prompts[0]["pid"], prompts[1]["pid"],
            "the Day's one session"
        );
        // Neither read the other's answer out of the shared journal.
        assert_eq!(rig.world.returns().len(), 4);
        let by_move = |name: &str| {
            rig.world.moves(&day_ref)["moves"][name]["last"]["end"]["gave"]["proposals"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!((by_move("tasks"), by_move("patterns")), (3, 1));
    }
}
