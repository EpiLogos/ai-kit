//! The Day's agent runs a move (manual 18.2, "Moves").
//!
//! Each of the Day's five pads is a move: Tasks (face 0 to 1), Patterns (2 to
//! 3), Context (2 to 4), Synthesis (1 to 4 into 5) and Close (5 to tomorrow's
//! 0). Central keeps what a move is: its prompt, when it runs, the record of
//! every run, which runs are due. Central never runs an agent
//! (`central.day.moves.read`, `central.day.move.record`,
//! `central.day.moves.due`, `central.day.close-carry`). This module is the
//! agent's side: it presses a move, takes one turn of the Day's own agent
//! session, and puts what the agent gave on the Day's faces as proposals that
//! wait for the person.
//!
//! What it does for one run (`run_move`):
//!
//! 1. reads the move's prompt and settings (`central.day.moves.read`) and the
//!    Day (`central.day.read`);
//! 2. records the begin (`central.day.move.record`). Central assigns the run
//!    and returns the prompt the run keeps, and the Agent writes setting the
//!    run began under; Off records the run refused and stops;
//! 3. takes ONE turn of the Day's session with Central's prompt, the faces'
//!    text and a strict answer shape for that move. The session is one
//!    persistent agent session for the World the Day belongs to, found by the
//!    label "the Day" and prepared once when absent ([`ResidentDayAgent`]).
//!    No provider configured: the run is refused in AIKit's words. Never a
//!    stub model;
//! 4. parses the answer strictly (an answer that does not parse stalls the
//!    run), and submits each entry as a `central.receiving.submit` proposal
//!    (`field.append`) on the face field it belongs to, authored by the Day's
//!    agent. Every proposal's id is derived from the run, so a retry cannot
//!    submit one twice. Central admits no direct agent write on a Day
//!    (`documents.rs`: "direct Agent mutation is not adoption"), so Write is
//!    held as Propose and the run says so;
//! 5. Close then calls `central.day.close-carry` and carries its answer;
//! 6. records the end (`done`, `refused` or `stalled`) with what it gave. A
//!    run that was begun is always ended. What it records is plain text
//!    (`plain_record`: nothing of the set Central refuses there), and an end
//!    Central refuses all the same ends the run refused, in Central's words.
//!
//! Nothing here runs by itself: no timer, no daemon, no routine. The native
//! host's visible runner calls `aikit work day-move due`, and the person's
//! pad calls `aikit work day-move run`.
#[cfg(unix)]
use crate::encounter_resident_turn::{OwnerError, ResidentOwner, SubmitStage, TurnWords};
use crate::encounter_service::EncounterRequest;
use aikit_adapters::runner::{CommandRunner, SystemRunner};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{AikitHome, ContextLock, LockOptions, SessionSpaceApplicationStore};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn fail(code: &'static str, message: impl Into<String>) -> AikitError {
    AikitError::new(code, message)
}

/// The sentence a run gives when Agent writes is Write: Central admits no
/// direct agent write on a Day, so the entries are proposals all the same.
pub const WRITE_HELD_AS_PROPOSE: &str =
    "Write is held as Propose: Central admits no direct agent write on a Day.";
/// The reason a run gives when Agent writes is Off.
pub const AGENT_WRITES_OFF: &str = "Agent writes is Off: the move would write nothing";
/// AIKit's words when no encounter provider is configured.
pub const NO_PROVIDER: &str = "no encounter provider is configured: the Day's agent has no harness to take a turn on. Configure one (`aikit session-space encounter-configure`, or `encounter-derive` for a harness profile) and press the move again";

const SCHEMA: &str = "aikit.day-move-run/v1";
/// Most entries one answer may carry, and the longest one entry may be.
const MAX_ENTRIES: usize = 128;
const MAX_ENTRY_BYTES: usize = 4000;
/// How much of one face's text a prompt carries.
const MAX_FACE_BYTES: usize = 12 * 1024;

// ---- the moves --------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Tasks,
    Patterns,
    Context,
    Synthesis,
    Close,
}

impl Move {
    pub const ALL: [Move; 5] = [
        Move::Tasks,
        Move::Patterns,
        Move::Context,
        Move::Synthesis,
        Move::Close,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Move::Tasks => "tasks",
            Move::Patterns => "patterns",
            Move::Context => "context",
            Move::Synthesis => "synthesis",
            Move::Close => "close",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Move::Tasks => "Tasks",
            Move::Patterns => "Patterns",
            Move::Context => "Context",
            Move::Synthesis => "Synthesis",
            Move::Close => "Close",
        }
    }

    pub fn parse(word: &str) -> Result<Move> {
        Move::ALL
            .into_iter()
            .find(|candidate| candidate.name() == word)
            .ok_or_else(|| {
                fail(
                    "day_move.move_unknown",
                    format!("a move is tasks, patterns, context, synthesis or close, not `{word}`"),
                )
            })
    }

    /// The faces the move reads (manual 18.2: Tasks reads face 0, Patterns and
    /// Context read what ran on face 2, Synthesis reads faces 1 to 4, Close
    /// reads face 5).
    fn reads(self) -> &'static [u8] {
        match self {
            Move::Tasks => &[0],
            Move::Patterns | Move::Context => &[2],
            Move::Synthesis => &[1, 2, 3, 4],
            Move::Close => &[5],
        }
    }

    /// The strict answer shape, in the words the agent receives.
    fn shape(self) -> &'static str {
        match self {
            Move::Tasks => {
                r#"{"must":["one task, one plain sentence"],"should":["…"],"could":["…"]}  (use [] for a tier with no task)"#
            }
            Move::Patterns => {
                r#"[{"pattern":"the pattern, in a phrase","items":["a thing from the day's sessions that shows it"]}]"#
            }
            Move::Context => {
                r#"[{"kind":"moment"|"person"|"place"|"file","text":"what it is","ref":"optional: a path, a link or a session ref"}]"#
            }
            Move::Synthesis => {
                r#"{"taught":"what the day taught","open":"what stays open","tomorrow":"a proposal for tomorrow's face 0"}  (use "" for a part you have nothing for)"#
            }
            Move::Close => {
                r#"{"summary":"the day, summarised","learned":["one thing the person learned"]}"#
            }
        }
    }

    /// The face fields where the move can land at all: a Day document that maps
    /// none of them leaves the move nowhere to put anything.
    fn any_of(self) -> &'static [&'static str] {
        match self {
            Move::Tasks => &["/fields/p1_must", "/fields/p1_should", "/fields/p1_could"],
            Move::Patterns => &["/fields/p3_patterns_noticed"],
            Move::Context => &[
                "/fields/p4_files_touched",
                "/fields/p4_people_concepts",
                "/fields/p4_timeline",
                "/body/p4_moments",
            ],
            Move::Synthesis | Move::Close => &["/fields/p5_synthesis"],
        }
    }
}

// ---- Central ------------------------------------------------------------------

/// A refusal or an absence from Central, in Central's words.
#[derive(Clone, Debug)]
pub struct CentralError {
    pub code: String,
    pub message: String,
    /// `ctrl` could not be run or did not answer: nothing was decided.
    pub unavailable: bool,
    /// What Central's refusal carries beside its words (`error.details`), or
    /// null: for a run that has ended already, the run as it ended.
    pub details: Value,
}

impl std::fmt::Display for CentralError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn central_failure(error: CentralError) -> AikitError {
    fail(
        if error.unavailable {
            "day_move.central_unavailable"
        } else {
            "day_move.central_refused"
        },
        error.message,
    )
}

/// Central's Actions, as the runner calls them.
pub trait CentralPort {
    fn call(&self, action: &str, input: &Value) -> std::result::Result<Value, CentralError>;
}

/// The real `ctrl`, run as `ctrl --json --root <root> action run <action> <input>`.
/// The credential the authenticated Actions need is the host's
/// `CENTRAL_NATIVE_TOKEN`, which the child inherits; it is never an input.
pub struct CtrlCentral {
    pub ctrl: PathBuf,
    pub root: PathBuf,
    /// A Project's Day instead of the root's.
    pub project: Option<String>,
    /// A credential held in hand (a test's, a host's) in place of the inherited
    /// `CENTRAL_NATIVE_TOKEN`. The command line never sets it.
    pub token: Option<String>,
}

impl CentralPort for CtrlCentral {
    fn call(&self, action: &str, input: &Value) -> std::result::Result<Value, CentralError> {
        let mut input = input.clone();
        if let (Some(project), Some(object)) = (&self.project, input.as_object_mut()) {
            object
                .entry("project")
                .or_insert_with(|| Value::String(project.clone()));
        }
        let unavailable = |message: String| CentralError {
            code: "day_move.central_unavailable".into(),
            message,
            unavailable: true,
            details: Value::Null,
        };
        let argv = vec![
            self.ctrl.to_string_lossy().into_owned(),
            "--json".into(),
            "--root".into(),
            self.root.to_string_lossy().into_owned(),
            "action".into(),
            "run".into(),
            action.into(),
            input.to_string(),
        ];
        let mut runner = SystemRunner::new().with_timeout(Duration::from_secs(120));
        if let Some(token) = &self.token {
            runner = runner.with_env("CENTRAL_NATIVE_TOKEN", token.clone());
        }
        let output = runner
            .run(&argv)
            .map_err(|error| unavailable(format!("{action}: {}", error.message())))?;
        let envelope: Value = serde_json::from_str(output.stdout.trim()).map_err(|_| {
            unavailable(format!(
                "{action}: ctrl answered no JSON envelope (exit {}){}",
                output.status,
                output
                    .stderr
                    .lines()
                    .last()
                    .map(|line| format!(": {line}"))
                    .unwrap_or_default()
            ))
        })?;
        if envelope["ok"] == true {
            return Ok(envelope["data"].clone());
        }
        Err(CentralError {
            code: envelope["error"]["code"]
                .as_str()
                .unwrap_or("central.refused")
                .to_owned(),
            message: envelope["error"]["message"]
                .as_str()
                .unwrap_or("Central refused the Action")
                .to_owned(),
            unavailable: false,
            details: envelope["error"]["details"].clone(),
        })
    }
}

// ---- the Day's agent ----------------------------------------------------------

/// Why the agent took no turn.
#[derive(Clone, Debug)]
pub enum TurnFailure {
    /// The turn could not begin because nothing is configured to take it. The
    /// run is refused, in the words given.
    Unconfigured(String),
    /// The turn began and did not give an answer. The run stalls.
    Failed(String),
}

/// The Day's agent: one turn in, the agent's whole answer out.
pub trait DayAgent {
    /// The ref proposals are authored under.
    fn agent_ref(&self, world_ref: &str) -> String;
    /// The session proposals are attributed to.
    fn session_ref(&self, world_ref: &str) -> Option<String>;
    fn turn(&self, world_ref: &str, prompt: &str) -> std::result::Result<String, TurnFailure>;
}

/// The Day's own session for a World: a SessionSpace labelled "the Day" and the
/// agent session in it, both named from the World so the same World always
/// finds the same session.
pub fn day_session(world_ref: &str) -> (SessionSpaceRef, ResourceRef) {
    let key = blake3::hash(world_ref.as_bytes()).to_hex();
    let key = &key.as_str()[..24];
    (
        SessionSpaceRef::parse(&format!("session-space/the-day-{key}"))
            .expect("a hashed ref is a valid ref"),
        ResourceRef::parse(format!("agent-session/the-day-{key}"))
            .expect("a hashed ref is a valid ref"),
    )
}

/// The Day's agent over AIKit's resident encounter owner: the same
/// `Draft` / `Prompt` / `Read` acts the task executor submits a turn with
/// (`encounter_task_executor.rs`), on the Day's own persistent session.
pub struct ResidentDayAgent {
    pub home: AikitHome,
    /// The directory the provider is opened in: the Central root.
    pub cwd: PathBuf,
    /// The encounter provider to take the turn on; absent, the one configured.
    pub provider: Option<String>,
    /// The ref proposals are authored under; absent, derived from the World.
    pub agent_ref: Option<String>,
    /// How long one turn may take before it is cancelled.
    pub timeout: Duration,
}

impl DayAgent for ResidentDayAgent {
    fn agent_ref(&self, world_ref: &str) -> String {
        self.agent_ref
            .clone()
            .unwrap_or_else(|| format!("agent:the-day:{world_ref}"))
    }

    fn session_ref(&self, world_ref: &str) -> Option<String> {
        Some(day_session(world_ref).1.to_string())
    }

    #[cfg(unix)]
    fn turn(&self, world_ref: &str, prompt: &str) -> std::result::Result<String, TurnFailure> {
        self.take_turn(world_ref, prompt)
    }

    #[cfg(not(unix))]
    fn turn(&self, _: &str, _: &str) -> std::result::Result<String, TurnFailure> {
        Err(TurnFailure::Failed(
            "the encounter owner needs a unix platform".into(),
        ))
    }
}

/// The words the Day's turns speak in (`encounter_resident_turn.rs`).
#[cfg(unix)]
const DAY_TURN: TurnWords<'static> = TurnWords {
    session: "the Day's session",
    act: "a Day move",
};

#[cfg(unix)]
impl ResidentDayAgent {
    fn owner(&self, request: &EncounterRequest) -> std::result::Result<Value, OwnerError> {
        self.resident().call(request)
    }

    fn resident(&self) -> ResidentOwner {
        ResidentOwner::at(
            self.home.clone(),
            self.cwd.clone(),
            crate::encounter_service::socket_path(&self.home),
        )
    }

    fn failed(what: &str, error: OwnerError) -> TurnFailure {
        TurnFailure::Failed(format!("{what}: {}", error.message))
    }

    /// The provider the Day's turn is taken on.
    fn chosen_provider(&self) -> std::result::Result<String, TurnFailure> {
        let rows = self
            .owner(&EncounterRequest::Providers)
            .map_err(|error| Self::failed("the encounter providers could not be read", error))?;
        let ids: Vec<String> = rows
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| row["id"].as_str().map(str::to_owned))
            .collect();
        let named = self
            .provider
            .clone()
            .or_else(|| std::env::var("AIKIT_DAY_PROVIDER").ok())
            .filter(|name| !name.is_empty());
        match (named, ids.len()) {
            (_, 0) => Err(TurnFailure::Unconfigured(NO_PROVIDER.into())),
            (Some(name), _) if ids.contains(&name) => Ok(name),
            (Some(name), _) => Err(TurnFailure::Unconfigured(format!(
                "the encounter provider `{name}` is not configured (configured: {})",
                ids.join(", ")
            ))),
            (None, 1) => Ok(ids[0].clone()),
            (None, _) => Err(TurnFailure::Unconfigured(format!(
                "several encounter providers are configured ({}); name the Day's with --provider",
                ids.join(", ")
            ))),
        }
    }

    /// Find the Day's session; prepare it once when absent, with the SessionSpace
    /// mutations `session-space agent-session-prepare` stages for any prepared
    /// session (a labelled space and the agent session attached to it).
    fn prepared_session(&self, world_ref: &str) -> Result<(SessionSpaceRef, ResourceRef)> {
        let (space, session) = day_session(world_ref);
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        let existing = match store.load(&space) {
            Ok(state) => Some(state),
            Err(error) if error.code() == "session_space.not_found" => None,
            Err(error) => return Err(error),
        };
        if existing.is_none() {
            let created = store
                .stage(
                    None,
                    SessionSpaceMutation::Create {
                        id: space.clone(),
                        label: Some("the Day".into()),
                    },
                )
                .and_then(|preview| store.apply(&preview));
            // Another runner may have prepared it first: that is still found.
            if let Err(error) = created {
                store.load(&space).map_err(|_| error)?;
            }
        }
        let attached = store.load(&space)?.agent_sessions.contains_key(&session);
        if !attached {
            let preview = store.stage(
                Some(&space),
                SessionSpaceMutation::AttachAgentSession {
                    attachment: SessionSpaceAgentAttachmentIntent {
                        agent_session: session.clone(),
                        purpose: Some(format!("the Day's moves for {world_ref}")),
                        provenance: vec![format!(
                            "prepared by `aikit work day-move run` for {world_ref}; the Day's agent, not an Agency grant"
                        )],
                    },
                },
            )?;
            store.apply(&preview)?;
        }
        Ok((space, session))
    }

    fn take_turn(&self, world_ref: &str, prompt: &str) -> std::result::Result<String, TurnFailure> {
        // The Day has one agent session and it takes one turn at a time: a second
        // move waits for the first (its run shows running meanwhile) rather than
        // reading the first's answer out of the shared journal.
        let key = blake3::hash(world_ref.as_bytes()).to_hex();
        let _one_turn = ContextLock::acquire(
            &self.home,
            &format!("day-move-turn-{}", &key.as_str()[..24]),
            LockOptions::default()
                .with_timeout(self.timeout + Duration::from_secs(60))
                .with_purpose("the Day's agent takes a turn"),
        )
        .map_err(|error| {
            TurnFailure::Failed(format!(
                "another move holds the Day's agent and did not finish in time: {}",
                error.message()
            ))
        })?;
        // The owner is asked through the one start path (ResidentOwner), which starts it when none
        // answers.
        self.owner(&EncounterRequest::Health).map_err(|error| {
            TurnFailure::Failed(format!(
                "the encounter owner is not running and could not be started: {}",
                error.message
            ))
        })?;
        let provider = self.chosen_provider()?;
        let (space, session) = self.prepared_session(world_ref).map_err(|error| {
            TurnFailure::Failed(format!(
                "the Day's session could not be prepared: {}",
                error.message()
            ))
        })?;
        let open = EncounterRequest::Open {
            space: space.clone(),
            agent_session: session.clone(),
            provider: provider.clone(),
            cwd: self.cwd.clone(),
        };
        match self.owner(&open) {
            Ok(_) => {}
            // The owner restarted since the session last ran: resume the one
            // recorded native session, never a replacement.
            Err(error) if error.code == "encounter.resume_required" => {
                self.owner(&EncounterRequest::Reconnect {
                    space,
                    agent_session: session.clone(),
                    provider,
                    cwd: self.cwd.clone(),
                })
                .map_err(|error| Self::failed("the Day's session could not be resumed", error))?;
            }
            Err(error) => return Err(Self::failed("the Day's session could not be opened", error)),
        }
        let baseline = self
            .resident()
            .submit(&session, prompt)
            .map_err(|(stage, error)| {
                Self::failed(
                    match stage {
                        SubmitStage::Read => "the Day's session could not be read",
                        SubmitStage::Draft => "the move's prompt could not be drafted",
                        SubmitStage::Prompt => "the move's prompt was not accepted",
                    },
                    error,
                )
            })?;
        let answer = self
            .resident()
            .await_turn(&session, baseline, self.timeout, DAY_TURN)
            .map_err(TurnFailure::Failed)?;
        if answer.said.trim().is_empty() {
            return Err(TurnFailure::Failed(
                "the agent's turn ended without an answer".into(),
            ));
        }
        Ok(answer.said)
    }
}

// ---- reading the Day ----------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DayDocument {
    pub source_ref: String,
    pub document_id: String,
    pub revision: String,
    /// The document's field map: `(field id, template pointer)`.
    pub fields: Vec<(String, String)>,
    pub payload: Value,
    pub contributions: Vec<Value>,
}

impl DayDocument {
    /// The document's field at the first of these template pointers it maps.
    fn field_at(&self, pointers: &[&str]) -> Option<(usize, &str)> {
        pointers.iter().enumerate().find_map(|(rank, pointer)| {
            self.fields
                .iter()
                .find(|(_, mapped)| mapped == pointer)
                .map(|(id, _)| (rank, id.as_str()))
        })
    }
}

#[derive(Clone, Debug)]
pub struct DayReading {
    pub day_ref: String,
    pub civil_date: String,
    pub lifecycle: String,
    /// The Day source's revision and the relations' revision: what
    /// `central.day.close-carry` is called on.
    pub revision: String,
    pub relations_revision: String,
    pub document: Option<DayDocument>,
}

fn read_day(
    central: &dyn CentralPort,
    day_ref: &str,
) -> std::result::Result<DayReading, CentralError> {
    let day = central.call("central.day.read", &json!({"day_ref": day_ref}))?;
    let text = |value: &Value| value.as_str().unwrap_or_default().to_owned();
    let document = match day["document"].as_object() {
        None => None,
        Some(document) => {
            let body = &document["document"];
            Some(DayDocument {
                source_ref: text(&document["source"]["ref"]),
                document_id: text(&document["document_id"]),
                revision: text(&document["revision"]["revision"]),
                fields: body["fields"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|field| {
                        Some((
                            field["id"].as_str()?.to_owned(),
                            field["template_pointer"].as_str()?.to_owned(),
                        ))
                    })
                    .collect(),
                payload: body["template_payload"].clone(),
                contributions: body["contributions"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default(),
            })
        }
    };
    Ok(DayReading {
        day_ref: text(&day["day_ref"]),
        civil_date: text(&day["temporal"]["civil_date"]),
        lifecycle: text(&day["temporal"]["lifecycle"]),
        revision: text(&day["revision"]["revision"]),
        relations_revision: text(&day["relations_revision"]),
        document,
    })
}

// ---- the faces' text ------------------------------------------------------------

const FACE_NAMES: [(&str, &str); 6] = [
    (
        "Ground",
        "what the person wants from the day, what worries them, what arrived",
    ),
    ("Definition", "the tasks, in three tiers"),
    ("Operation", "the sessions of the day and what ran"),
    ("Pattern", "the patterns across the day's work"),
    (
        "Context",
        "where the day sat: moments, people, places, files",
    ),
    (
        "Synthesis",
        "what the day taught, what stays open, tomorrow",
    ),
];

/// The face's fields, by key; the body keys it holds; the document's top-level
/// lists it holds (the page template's `ql-doc`: ql-daily-die.html).
fn face_keys(
    face: u8,
) -> (
    &'static [&'static str],
    &'static [&'static str],
    &'static [&'static str],
) {
    match face {
        0 => (&["p0_quick_thoughts", "p0_adjacencies"], &[], &["capture"]),
        1 => (
            &["p1_intentions", "p1_must", "p1_should", "p1_could"],
            &[],
            &["completed"],
        ),
        2 => (&["p2_operations", "p2_manual_activity"], &[], &["sessions"]),
        3 => (
            &[
                "p3_patterns_noticed",
                "p3_recurring_themes",
                "p3_connections_made",
            ],
            &["p3_observations"],
            &[],
        ),
        4 => (
            &[
                "p4_files_touched",
                "p4_people_concepts",
                "p4_graph_additions",
            ],
            &["p4_moments"],
            &[],
        ),
        _ => (
            &["p5_synthesis", "p5_quintessence", "p5_teleological_aim"],
            &[
                "p5_learning_1",
                "p5_learning_2",
                "p5_learning_3",
                "p5_blockers",
                "p5_tomorrow",
            ],
            &[],
        ),
    }
}

/// Visible text of a piece of the page's HTML: tags dropped, block ends kept as
/// line ends.
fn plain_text(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| !c.is_ascii_alphanumeric())
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "br" | "p" | "li" | "div" | "tr" | "h1" | "h2" | "h3"
                ) && !out.ends_with('\n')
                {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
        .trim()
        .to_owned()
}

/// A page value as text: a string is its visible text, a list is its entries one
/// to a line, an object is its parts.
fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => plain_text(text),
        Value::Array(rows) => rows
            .iter()
            .map(value_text)
            .filter(|row| !row.is_empty())
            .map(|row| format!("- {}", row.replace('\n', " ")))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(parts) => parts
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "id" | "uid"))
            .map(|(key, part)| (key, value_text(part)))
            .filter(|(_, part)| !part.is_empty())
            .map(|(key, part)| format!("{key}: {}", part.replace('\n', " ")))
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    }
}

fn bounded(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[…cut]");
    }
    text
}

/// One face as the agent reads it: each of its fields, the page's own lists, and
/// the entries already contributed to it with their authors.
fn face_text(document: &DayDocument, face: u8) -> String {
    let (fields, body, lists) = face_keys(face);
    let (name, holds) = FACE_NAMES[usize::from(face.min(5))];
    let mut out = format!("Face {face}, {name} ({holds}):\n");
    let mut any = false;
    let mut put = |label: &str, text: String, out: &mut String| {
        if !text.trim().is_empty() {
            any = true;
            out.push_str(&format!("  {label}: {}\n", text.replace('\n', "\n    ")));
        }
    };
    for key in fields.iter().copied() {
        put(key, value_text(&document.payload["fields"][key]), &mut out);
    }
    for key in body.iter().copied() {
        put(key, value_text(&document.payload["body"][key]), &mut out);
    }
    for key in lists.iter().copied() {
        put(key, value_text(&document.payload[key]), &mut out);
    }
    for key in fields.iter().chain(body.iter()) {
        let Some((id, _)) = document.fields.iter().find(|(_, pointer)| {
            pointer == &format!("/fields/{key}") || pointer == &format!("/body/{key}")
        }) else {
            continue;
        };
        for entry in document
            .contributions
            .iter()
            .filter(|entry| entry["field_id"] == id.as_str() && entry["removed"] != true)
        {
            put(
                &format!(
                    "{key} (added by {})",
                    entry["author_ref"].as_str().unwrap_or("unknown")
                ),
                value_text(&entry["html"]),
                &mut out,
            );
        }
    }
    if !any {
        out.push_str("  (nothing written on this face)\n");
    }
    bounded(out, MAX_FACE_BYTES)
}

fn compose_prompt(
    mv: Move,
    central_prompt: &str,
    day: &DayReading,
    document: &DayDocument,
    world_ref: &str,
) -> String {
    let faces = mv
        .reads()
        .iter()
        .map(|face| face_text(document, *face))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{central_prompt}\n\n---\nThe Day of {date} ({world_ref}), the {label} move. You are the Day's agent. Take no action and use no tool: your answer is read by AIKit, and each entry in it becomes a proposal that waits for the person to accept or decline it.\n\n{faces}\n---\nAnswer with exactly one JSON document of this shape and nothing else: no prose before or after it, no code fence.\nShape: {shape}\n",
        date = day.civil_date,
        label = mv.label(),
        shape = mv.shape(),
    )
}

// ---- the answer -----------------------------------------------------------------

/// One thing the agent gave, before it has a field.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    /// The kind of entry, for counting (`must`, `pattern`, `file`, `taught`, …).
    kind: &'static str,
    /// The template pointers it can land on, best first.
    pointers: &'static [&'static str],
    /// Said first when it lands anywhere but on the best pointer.
    lead: Option<&'static str>,
    html: String,
    /// A short plain line, for the receiving record's summary.
    brief: String,
}

#[derive(Debug, Default)]
struct Answer {
    entries: Vec<Entry>,
    /// What the agent says the person learned (Close): counted, not proposed.
    learned: usize,
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Text as paragraphs: blank lines separate paragraphs, a single line break is a
/// line break.
fn paragraphs(text: &str) -> String {
    text.trim()
        .split("\n\n")
        .map(|paragraph| format!("<p>{}</p>", esc(paragraph.trim()).replace('\n', "<br>")))
        .collect()
}

fn brief(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    bounded(line, 160)
}

/// The agent's whole answer as one JSON document. A single surrounding code
/// fence is formatting, not content; prose around the document is neither.
fn answer_json(text: &str) -> std::result::Result<Value, String> {
    let mut body = text.trim();
    if let Some(rest) = body.strip_prefix("```") {
        let rest = rest.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
        body = rest.trim_end().strip_suffix("```").unwrap_or(rest).trim();
    }
    serde_json::from_str(body).map_err(|error| {
        format!(
            "it is not one JSON document ({error}); it began: {:?}",
            body.chars().take(60).collect::<String>()
        )
    })
}

fn strings(value: &Value, what: &str) -> std::result::Result<Vec<String>, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| format!("{what} is a list of text"))?;
    rows.iter()
        .map(|row| entry_text(row, what))
        .collect::<std::result::Result<Vec<_>, _>>()
}

fn entry_text(value: &Value, what: &str) -> std::result::Result<String, String> {
    let text = value
        .as_str()
        .ok_or_else(|| format!("{what} holds only text"))?
        .trim();
    if text.is_empty() {
        return Err(format!("{what} holds an empty entry"));
    }
    if text.len() > MAX_ENTRY_BYTES {
        return Err(format!(
            "{what} holds an entry over {MAX_ENTRY_BYTES} bytes"
        ));
    }
    Ok(text.to_owned())
}

fn only_keys(object: &Map<String, Value>, allowed: &[&str]) -> std::result::Result<(), String> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("it has a `{key}`, which the shape does not"));
        }
    }
    for key in allowed {
        if !object.contains_key(*key) {
            return Err(format!("it has no `{key}`"));
        }
    }
    Ok(())
}

/// Where an entry can land: template pointers of the page's `ql-doc`, best first.
type Pointers = &'static [&'static str];
const MUST: Pointers = &["/fields/p1_must"];
const SHOULD: Pointers = &["/fields/p1_should"];
const COULD: Pointers = &["/fields/p1_could"];
const PATTERNS: Pointers = &["/fields/p3_patterns_noticed"];
const FILES: Pointers = &["/fields/p4_files_touched"];
const PEOPLE: Pointers = &["/fields/p4_people_concepts"];
const MOMENTS: Pointers = &["/fields/p4_timeline", "/body/p4_moments"];
const SYNTHESIS: Pointers = &["/fields/p5_synthesis"];
const OPEN: Pointers = &["/body/p5_blockers", "/fields/p5_synthesis"];
const TOMORROW: Pointers = &["/body/p5_tomorrow", "/fields/p5_synthesis"];

fn parse_answer(mv: Move, text: &str) -> std::result::Result<Answer, String> {
    let value = answer_json(text)?;
    let mut answer = Answer::default();
    match mv {
        Move::Tasks => {
            let object = value
                .as_object()
                .ok_or("it is not an object {must, should, could}")?;
            only_keys(object, &["must", "should", "could"])?;
            for (tier, pointers) in [("must", MUST), ("should", SHOULD), ("could", COULD)] {
                for task in strings(&object[tier], tier)? {
                    answer.entries.push(Entry {
                        kind: tier,
                        pointers,
                        lead: None,
                        html: format!("<p>{}</p>", esc(&task)),
                        brief: brief(&task),
                    });
                }
            }
        }
        Move::Patterns => {
            for row in value
                .as_array()
                .ok_or("it is not a list of {pattern, items}")?
            {
                let object = row
                    .as_object()
                    .ok_or("a pattern is not an object {pattern, items}")?;
                only_keys(object, &["pattern", "items"])?;
                let pattern = entry_text(&object["pattern"], "a pattern")?;
                let items = strings(&object["items"], "a pattern's items")?;
                if items.is_empty() {
                    return Err(format!(
                        "the pattern `{}` has no item that shows it",
                        brief(&pattern)
                    ));
                }
                answer.entries.push(Entry {
                    kind: "pattern",
                    pointers: PATTERNS,
                    lead: None,
                    html: format!(
                        "<p><strong>{}</strong></p><ul>{}</ul>",
                        esc(&pattern),
                        items
                            .iter()
                            .map(|item| format!("<li>{}</li>", esc(item)))
                            .collect::<String>()
                    ),
                    brief: brief(&pattern),
                });
            }
        }
        Move::Context => {
            for row in value.as_array().ok_or("it is not a list of {kind, text}")? {
                let object = row
                    .as_object()
                    .ok_or("an entry is not an object {kind, text}")?;
                for key in object.keys() {
                    if !matches!(key.as_str(), "kind" | "text" | "ref") {
                        return Err(format!("an entry has a `{key}`, which the shape does not"));
                    }
                }
                let kind = object
                    .get("kind")
                    .and_then(Value::as_str)
                    .ok_or("an entry has no `kind`")?;
                let (kind, pointers, label): (&'static str, Pointers, &str) = match kind {
                    "file" => ("file", FILES, "File"),
                    "person" => ("person", PEOPLE, "Person"),
                    "place" => ("place", PEOPLE, "Place"),
                    "moment" => ("moment", MOMENTS, "Moment"),
                    other => {
                        return Err(format!(
                            "an entry's kind is moment, person, place or file, not `{other}`"
                        ))
                    }
                };
                let text = entry_text(
                    object.get("text").ok_or("an entry has no `text`")?,
                    "an entry",
                )?;
                let reference = match object.get("ref") {
                    None | Some(Value::Null) => String::new(),
                    Some(reference) => {
                        format!(" <em>({})</em>", esc(&entry_text(reference, "a ref")?))
                    }
                };
                answer.entries.push(Entry {
                    kind,
                    pointers,
                    lead: None,
                    html: format!("<p><strong>{label}</strong> {}{reference}</p>", esc(&text)),
                    brief: brief(&text),
                });
            }
        }
        Move::Synthesis => {
            let object = value
                .as_object()
                .ok_or("it is not an object {taught, open, tomorrow}")?;
            only_keys(object, &["taught", "open", "tomorrow"])?;
            for (key, pointers, lead) in [
                ("taught", SYNTHESIS, None),
                ("open", OPEN, Some("Still open")),
                ("tomorrow", TOMORROW, Some("For tomorrow")),
            ] {
                let text = object[key]
                    .as_str()
                    .ok_or_else(|| format!("`{key}` is text"))?
                    .trim();
                if text.len() > MAX_ENTRY_BYTES * 4 {
                    return Err(format!("`{key}` is over {} bytes", MAX_ENTRY_BYTES * 4));
                }
                if !text.is_empty() {
                    answer.entries.push(Entry {
                        kind: key,
                        pointers,
                        lead,
                        html: paragraphs(text),
                        brief: brief(text),
                    });
                }
            }
        }
        Move::Close => {
            let object = value
                .as_object()
                .ok_or("it is not an object {summary, learned}")?;
            only_keys(object, &["summary", "learned"])?;
            let summary = object["summary"]
                .as_str()
                .ok_or("`summary` is text")?
                .trim();
            if summary.is_empty() {
                return Err("`summary` is empty".into());
            }
            answer.entries.push(Entry {
                kind: "summary",
                pointers: SYNTHESIS,
                lead: Some("The day, summarised"),
                html: paragraphs(summary),
                brief: brief(summary),
            });
            answer.learned = strings(&object["learned"], "learned")?.len();
        }
    }
    if answer.entries.len() > MAX_ENTRIES {
        return Err(format!("it holds more than {MAX_ENTRIES} entries"));
    }
    Ok(answer)
}

// ---- proposals -------------------------------------------------------------------

/// The id of one proposal: the producer key of its `central.receiving.submit`
/// and the contribution id inside it. It is derived from the run, the field and
/// the entry, so the same run asked again names the same proposal.
pub fn proposal_key(run_ref: &str, field_id: &str, html: &str) -> String {
    let digest = blake3::hash(html.as_bytes()).to_hex();
    format!("{run_ref}#{field_id}#{}", &digest.as_str()[..12])
}

struct Resolved {
    field_id: String,
    html: String,
    brief: String,
    kind: &'static str,
}

/// Put each entry on a field the Day document maps. An entry no mapped field can
/// take is returned with the pointers it wanted, never put somewhere else.
fn resolve(entries: &[Entry], document: &DayDocument) -> (Vec<Resolved>, Vec<(Entry, String)>) {
    let mut resolved = Vec::new();
    let mut unmapped = Vec::new();
    for entry in entries {
        match document.field_at(entry.pointers) {
            Some((rank, field_id)) => resolved.push(Resolved {
                field_id: field_id.to_owned(),
                html: match (rank, entry.lead) {
                    (rank, Some(lead)) if rank > 0 || entry.kind == "summary" => {
                        format!("<p><strong>{lead}</strong></p>{}", entry.html)
                    }
                    _ => entry.html.clone(),
                },
                brief: entry.brief.clone(),
                kind: entry.kind,
            }),
            None => unmapped.push((entry.clone(), entry.pointers.join(" or "))),
        }
    }
    (resolved, unmapped)
}

struct Submitter<'a> {
    central: &'a dyn CentralPort,
    agent_ref: String,
    session_ref: Option<String>,
    day_ref: String,
    run_ref: String,
    label: &'static str,
}

impl Submitter<'_> {
    fn input(&self, producer_key: &str, target: &Value, brief: &str) -> Value {
        let mut input = json!({
            "producer_key": producer_key,
            "source_ref": target["source_ref"],
            "document_id": target["document_id"],
            "expected_source_revision": target["expected_source_revision"],
            "proposal": target["proposal"],
            "summary": format!("{}: {brief}", self.label),
            "declared_producer": {"ref": self.agent_ref, "actor_kind": "agent", "attribution": "claimed"},
            "day_ref": target["day_ref"],
            "run_ref": self.run_ref,
        });
        if let Some(session) = &self.session_ref {
            input["session_ref"] = json!(session);
        }
        input
    }

    /// Submit one proposal; a proposal that Central already holds under this key
    /// (a retry, or a lost answer) is the same proposal, returned again. The
    /// answer is the ref Central named for it, or none when Central took it and
    /// named none: it is submitted all the same (be2j.a2 item 6).
    fn submit(
        &self,
        producer_key: &str,
        target: &Value,
        brief: &str,
    ) -> std::result::Result<Option<String>, CentralError> {
        let input = self.input(producer_key, target, brief);
        let submitted = self.central.call("central.receiving.submit", &input);
        match submitted {
            Ok(received) => Ok(named_ref(&received)),
            Err(error) if !error.unavailable => {
                match self.central.call(
                    "central.receiving.read",
                    &json!({"producer_key": producer_key}),
                ) {
                    Ok(held) if held["return_ref"].is_string() => Ok(named_ref(&held)),
                    _ => Err(error),
                }
            }
            Err(error) => Err(error),
        }
    }

    fn entry(
        &self,
        document: &DayDocument,
        resolved: &Resolved,
    ) -> std::result::Result<Option<String>, CentralError> {
        let key = proposal_key(&self.run_ref, &resolved.field_id, &resolved.html);
        let target = json!({
            "source_ref": document.source_ref,
            "document_id": document.document_id,
            "expected_source_revision": document.revision,
            "day_ref": self.day_ref,
            "proposal": {"operation": "field.append", "field_id": resolved.field_id,
                "contribution_id": key, "html": resolved.html},
        });
        self.submit(&key, &target, &resolved.brief)
    }
}

/// The ref Central named for a proposal it took, when it named one.
fn named_ref(answer: &Value) -> Option<String> {
    answer["return_ref"]
        .as_str()
        .filter(|named| !named.trim().is_empty())
        .map(str::to_owned)
}

// ---- a run -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trigger {
    Press,
    Schedule,
}

impl Trigger {
    pub fn parse(word: &str) -> Result<Trigger> {
        match word {
            "press" => Ok(Trigger::Press),
            "schedule" => Ok(Trigger::Schedule),
            other => Err(fail(
                "day_move.trigger_unknown",
                format!("a trigger is press or schedule, not `{other}`"),
            )),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Trigger::Press => "press",
            Trigger::Schedule => "schedule",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RunRequest {
    pub mv: Move,
    pub day_ref: Option<String>,
    pub trigger: Trigger,
    pub occurrence_ref: Option<String>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// What a run gave, for `central.day.move.record`'s `gave`.
#[derive(Debug, Default)]
struct Gave {
    summary: String,
    proposals: Vec<String>,
    /// Proposals Central took and named no ref for: counted as proposed, and
    /// not listed, since there is no ref to list (be2j.a2 item 6).
    unnamed: usize,
    counts: Map<String, Value>,
}

impl Gave {
    /// Every proposal Central took, named or not.
    fn submitted(&self) -> usize {
        self.proposals.len() + self.unnamed
    }

    /// One more proposal Central took, under the ref it named for it.
    fn take(&mut self, named: Option<String>) {
        match named {
            Some(return_ref) => self.proposals.push(return_ref),
            None => self.unnamed += 1,
        }
    }

    fn json(&self) -> Value {
        json!({"summary": self.summary, "proposals": self.proposals, "counts": self.counts})
    }

    /// What the record says of the proposals Central named no ref for, or none.
    /// It is the record's own sentence, not the run's words: `plain_record`
    /// puts it after the summary and never cuts it (be2j.a4 item 3).
    fn says(&self) -> Option<String> {
        (self.unnamed > 0).then(|| {
            format!(
                "Central named no ref for {}, so this record does not list {}.",
                count_words(self.unnamed, "proposal", "proposals"),
                if self.unnamed == 1 { "it" } else { "them" }
            )
        })
    }
}

/// Why a run stopped short of done.
struct Stop {
    state: &'static str,
    reason: String,
    gave: Option<Gave>,
}

fn refused(reason: impl Into<String>) -> Stop {
    Stop {
        state: "refused",
        reason: reason.into(),
        gave: None,
    }
}

fn stalled(reason: impl Into<String>) -> Stop {
    Stop {
        state: "stalled",
        reason: reason.into(),
        gave: None,
    }
}

struct RunContext {
    mv: Move,
    day_ref: String,
    world_ref: String,
    run_ref: String,
    /// Central's effective prompt, the one the run kept at its begin.
    prompt: String,
    agent_writes: String,
}

fn count_words(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

fn execute(
    central: &dyn CentralPort,
    agent: &dyn DayAgent,
    context: &RunContext,
) -> std::result::Result<Gave, Stop> {
    let mv = context.mv;
    if context.agent_writes == "off" {
        return Err(refused(AGENT_WRITES_OFF));
    }
    let day = read_day(central, &context.day_ref)
        .map_err(|error| stalled(format!("the Day could not be read: {error}")))?;
    let document = day.document.as_ref().ok_or_else(|| {
        refused("The Day has no Day document yet, so there is no face to read and nothing to propose onto; open the Day's page first.")
    })?;
    if document.field_at(mv.any_of()).is_none() {
        return Err(refused(format!(
            "The Day document maps none of the fields {} can propose onto ({}); it maps {}.",
            mv.label(),
            mv.any_of().join(", "),
            if document.fields.is_empty() {
                "no fields".to_owned()
            } else {
                document
                    .fields
                    .iter()
                    .map(|(_, pointer)| pointer.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        )));
    }
    let prompt = compose_prompt(mv, &context.prompt, &day, document, &context.world_ref);
    let said = agent
        .turn(&context.world_ref, &prompt)
        .map_err(|failure| match failure {
            TurnFailure::Unconfigured(reason) => refused(reason),
            TurnFailure::Failed(reason) => stalled(reason),
        })?;
    let answer = parse_answer(mv, &said).map_err(|reason| {
        stalled(format!(
            "The agent's answer did not parse as {}'s shape: {reason}",
            mv.label()
        ))
    })?;
    // The person may have written while the agent thought: propose against the
    // Day as it stands now.
    let day = read_day(central, &context.day_ref)
        .map_err(|error| stalled(format!("the Day could not be read again: {error}")))?;
    let document = day
        .document
        .as_ref()
        .ok_or_else(|| stalled("The Day's document is gone; nothing was proposed."))?;
    let (resolved, unmapped) = resolve(&answer.entries, document);
    let submitter = Submitter {
        central,
        agent_ref: agent.agent_ref(&context.world_ref),
        session_ref: agent.session_ref(&context.world_ref),
        day_ref: context.day_ref.clone(),
        run_ref: context.run_ref.clone(),
        label: mv.label(),
    };
    let mut gave = Gave::default();
    let mut seen = BTreeSet::new();
    let mut duplicates = 0;
    let mut by_kind: Map<String, Value> = Map::new();
    for (position, item) in resolved.iter().enumerate() {
        if !seen.insert(proposal_key(&context.run_ref, &item.field_id, &item.html)) {
            duplicates += 1;
            continue;
        }
        match submitter.entry(document, item) {
            Ok(named) => {
                gave.take(named);
                let held = by_kind.get(item.kind).and_then(Value::as_u64).unwrap_or(0);
                by_kind.insert(item.kind.to_owned(), json!(held + 1));
            }
            Err(error) => {
                let reason = format!(
                    "Central refused proposal {} of {} ({}): {error}. {} already submitted wait for you.",
                    position + 1,
                    resolved.len(),
                    item.field_id,
                    count_words(gave.submitted(), "proposal", "proposals"),
                );
                gave.summary = format!(
                    "{}: {} submitted before Central refused one.",
                    mv.label(),
                    count_words(gave.submitted(), "proposal", "proposals")
                );
                gave.counts =
                    counts_of(&by_kind, &gave, unmapped.len(), duplicates, answer.learned);
                return Err(Stop {
                    state: "stalled",
                    reason,
                    gave: Some(gave),
                });
            }
        }
    }
    gave.counts = counts_of(&by_kind, &gave, unmapped.len(), duplicates, answer.learned);
    let entries = answer.entries.len();
    let mut summary = if entries == 0 {
        format!("{}: the agent proposed nothing.", mv.label())
    } else {
        format!(
            "{}: {} submitted for {}, each waiting for you on its face.",
            mv.label(),
            count_words(gave.submitted(), "proposal", "proposals"),
            count_words(entries, "entry", "entries"),
        )
    };
    if !unmapped.is_empty() {
        summary.push_str(&format!(
            " {} had no field on the Day document to land on and {} not proposed ({}).",
            count_words(unmapped.len(), "entry", "entries"),
            if unmapped.len() == 1 { "was" } else { "were" },
            unmapped
                .iter()
                .map(|(entry, wanted)| format!("{}: {wanted}", entry.kind))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    if context.agent_writes == "write" {
        summary.push(' ');
        summary.push_str(WRITE_HELD_AS_PROPOSE);
    }
    gave.summary = summary;
    if mv == Move::Close {
        close_step(central, &submitter, &day, &mut gave, answer.learned)?;
    }
    Ok(gave)
}

fn counts_of(
    by_kind: &Map<String, Value>,
    gave: &Gave,
    unmapped: usize,
    duplicates: usize,
    learned: usize,
) -> Map<String, Value> {
    let mut counts = by_kind.clone();
    counts.insert("proposed".into(), json!(gave.submitted()));
    counts.insert("unmapped".into(), json!(unmapped));
    if duplicates > 0 {
        counts.insert("duplicates".into(), json!(duplicates));
    }
    if learned > 0 {
        counts.insert("learned".into(), json!(learned));
    }
    counts
}

/// Close, after the agent's summary: close the Day and carry face 5 to tomorrow.
fn close_step(
    central: &dyn CentralPort,
    submitter: &Submitter<'_>,
    day: &DayReading,
    gave: &mut Gave,
    learned: usize,
) -> std::result::Result<(), Stop> {
    let partial = |gave: &Gave, summary: String| Gave {
        summary,
        proposals: gave.proposals.clone(),
        unnamed: gave.unnamed,
        counts: gave.counts.clone(),
    };
    let day = read_day(central, &day.day_ref)
        .map_err(|error| stalled(format!("the Day could not be read to close it: {error}")))?;
    let closed = central.call(
        "central.day.close-carry",
        &json!({"day_ref": day.day_ref, "expected_revision": day.revision,
            "expected_relations_revision": day.relations_revision}),
    );
    let closed = match closed {
        Ok(closed) => closed,
        Err(error) => {
            let summary = format!("{} The Day was not closed.", gave.summary.trim_end());
            return Err(Stop {
                state: "stalled",
                reason: format!("Central did not close the Day: {error}"),
                gave: Some(partial(gave, summary)),
            });
        }
    };
    let carry = &closed["carry"];
    let state = carry["state"].as_str().unwrap_or("unknown").to_owned();
    gave.counts.insert("carry_state".into(), json!(state));
    gave.counts.insert("day_closed".into(), json!(true));
    if let Some(reason) = carry["reason"].as_str().filter(|reason| !reason.is_empty()) {
        gave.counts.insert("carry_reason".into(), json!(reason));
    }
    let mut carried = format!("the carry to tomorrow's face 0: {state}");
    if state == "waits" {
        // An agent's entry into a Day waits for the person: Central hands back the
        // proposal ready to submit, and it is submitted here, or it would be lost
        // with the Day closed.
        let key = format!("{}#close-carry", submitter.run_ref);
        let target = json!({
            "source_ref": carry["proposal"]["source_ref"],
            "document_id": carry["proposal"]["document_id"],
            "expected_source_revision": carry["proposal"]["expected_source_revision"],
            "day_ref": closed["tomorrow"]["day_ref"],
            "proposal": carry["proposal"]["proposal"],
        });
        match submitter.submit(&key, &target, "face 5's proposal for tomorrow's face 0") {
            Ok(named) => {
                gave.take(named);
                carried.push_str(" (submitted as a proposal for tomorrow's face 0)");
            }
            Err(error) => {
                let summary = format!(
                    "{} The Day is closed; its carry to tomorrow's face 0 was not submitted.",
                    gave.summary.trim_end()
                );
                return Err(Stop {
                    state: "stalled",
                    reason: format!(
                        "The Day closed, but Central refused the carry proposal for tomorrow's face 0: {error}"
                    ),
                    gave: Some(partial(gave, summary)),
                });
            }
        }
    } else if let Some(reason) = carry["reason"].as_str().filter(|reason| !reason.is_empty()) {
        carried.push_str(&format!(" ({reason})"));
    }
    gave.counts
        .insert("proposed".into(), json!(gave.submitted()));
    let knowledge = if learned == 0 {
        "The agent named nothing the person learned.".to_owned()
    } else {
        format!(
            "{} the agent says the person learned {} not sent to their knowledge: AIKit has no knowledge-return verb for them, so no knowledge return was made.",
            count_words(learned, "thing", "things"),
            if learned == 1 { "was" } else { "were" }
        )
    };
    gave.summary = format!(
        "{} The Day is closed; {carried}. {knowledge}",
        gave.summary.trim_end()
    );
    Ok(())
}

/// Central's code for a record that met a newer state than the one it was
/// made against: the Day's moves changed under it (`action_result` in
/// `ctrl/src/continuous_work/mod.rs`, the `AlreadyExists` arm). A Central from
/// before `CENTRAL_RUN_ENDED` gives this code for a run that has ended already
/// too, so after it the run is read again to tell the two apart, never by
/// Central's words.
const CENTRAL_CONFLICT: &str = "stale_basis_or_identity_conflict";

/// Central's code for an end given to a run that has ended already
/// (`RUN_ENDED_ALREADY` in `ctrl/src/continuous_work/day_moves.rs`, be2j.c3):
/// the run is finished and nothing is given again. The refusal's details carry
/// the run as it ended, so the Day's moves are not read again to learn it
/// (be2j.a3 item 6). A Central of either age is answered rightly: this code by
/// what it says, `CENTRAL_CONFLICT` by the read.
const CENTRAL_RUN_ENDED: &str = "run_ended_already";

/// The state a report gives a run that stands ended at Central when how it
/// ended could not be read: a state of its own, and not an error, which a
/// caller would take for a run to try again (be2j.a3 item 2). It is said only
/// where the run is known ended: Central answered `CENTRAL_RUN_ENDED` for it.
/// A conflict alone never says so (be2j.a4 item 1).
const ENDED_NOT_READ: &str = "ended, state not read";

/// How a run stands at Central, by the Day's moves.
enum Stands {
    /// It has ended: the run as Central holds it.
    Ended(Value),
    Running,
    /// The Day's moves could not be read.
    NotRead,
}

/// A begun run is ended even when the runner stops without ending it (a panic,
/// an early return): it ends `stalled` and says so. A recorded end disarms it,
/// and so does finding the run ended already.
struct OpenRun<'a> {
    central: &'a dyn CentralPort,
    mv: Move,
    day_ref: String,
    run_ref: String,
    begun_at: i64,
    armed: bool,
    /// The run's own end was tried: what is left to the guard is one try, as
    /// `stalled`.
    end_tried: bool,
}

impl OpenRun<'_> {
    /// Record how the run ended, in plain text (`plain_record`). What answers
    /// a record Central refuses goes by the refusal's code, never by its words
    /// (be2j.a2 item 3), and where the run stands ended at Central the answer
    /// is that run and never an error (be2j.a3 item 2):
    /// - the run has ended already (`CENTRAL_RUN_ENDED`): it stays as it
    ///   ended, and no further end is recorded (`ended`);
    /// - Central did not answer: the end may have been taken and its answer
    ///   lost, so the run is read once. Ended, it is the report; else nothing
    ///   more is tried here, and the guard tries `stalled` once as the run is
    ///   dropped;
    /// - a conflict (`CENTRAL_CONFLICT`): the Day's moves are read again. A run
    ///   that has ended stays as it ended. A run still running had the Day
    ///   change under it, and the same end, with what the run gave, is
    ///   recorded once more; refused as a conflict again, the run is read once
    ///   more, since it may have ended between the read and that end;
    /// - any other refusal, and a conflict refused again while the run still
    ///   runs: the run ends `refused`, in Central's words and without what it
    ///   gave, so a begun run is never left unended over what it gave (be2j
    ///   ruling 19). Where that end is a conflict too, the run is read: ended,
    ///   it is the report; running or unread, it is not known ended, so the
    ///   answer is an error and the guard keeps its one try. `ENDED_NOT_READ`
    ///   is said only of a run Central named ended by its own code (`ended`;
    ///   be2j.a4 item 1).
    fn end(&mut self, state: &str, gave: Option<&Gave>, reason: Option<&str>) -> Result<Value> {
        self.end_tried = true;
        let mut refusal = match self.record_end(state, gave, reason) {
            Ok(ended) => return Ok(ended),
            Err(refusal) => refusal,
        };
        if !refusal.unavailable && refusal.code == CENTRAL_CONFLICT {
            if let Stands::Ended(run) = self.stands(2) {
                return Ok(self.as_ended(run));
            }
            refusal = match self.record_end(state, gave, reason) {
                Ok(ended) => return Ok(ended),
                Err(refusal) => refusal,
            };
            if !refusal.unavailable && refusal.code == CENTRAL_CONFLICT {
                if let Stands::Ended(run) = self.stands(2) {
                    return Ok(self.as_ended(run));
                }
            }
        }
        if let Some(settled) = self.settled(&refusal) {
            return settled;
        }
        let words = format!(
            "Central refused the record of how this run ended: {}",
            refusal.message
        );
        let refusal = match self.record_end("refused", None, Some(&words)) {
            Ok(ended) => return Ok(ended),
            Err(refusal) => refusal,
        };
        if let Some(settled) = self.settled(&refusal) {
            return settled;
        }
        // A conflict does not say the run has ended: a Central with be2j.c3 names an ended run by a code of its own, and
        // from an older one a conflict is also the Day changing. So only a read that shows the run ended makes it the
        // report here. Unread, the run is not known ended: an error, and the guard keeps its one try (be2j.a4 item 1).
        if refusal.code == CENTRAL_CONFLICT {
            if let Stands::Ended(run) = self.stands(2) {
                return Ok(self.as_ended(run));
            }
        }
        Err(central_failure(refusal))
    }

    /// What a refusal settles by itself, or none when another end is to be
    /// tried: a run that has ended already is the report; an answer that never
    /// came is the report when the run reads ended, and the error it was
    /// otherwise.
    fn settled(&mut self, refusal: &CentralError) -> Option<Result<Value>> {
        if refusal.unavailable {
            return Some(match self.stands(1) {
                Stands::Ended(run) => Ok(self.as_ended(run)),
                Stands::Running | Stands::NotRead => Err(central_failure(refusal.clone())),
            });
        }
        (refusal.code == CENTRAL_RUN_ENDED).then(|| Ok(self.ended(refusal)))
    }

    /// The report of a run Central says has ended already: the run as the
    /// refusal carries it; from a Central that carries none, the run as the
    /// Day's moves hold it; and where that cannot be read, `ENDED_NOT_READ`.
    /// The run is finished whichever it is, so the guard is disarmed.
    fn ended(&mut self, refusal: &CentralError) -> Value {
        let carried = refusal.details.get("run").filter(|run| {
            run["run_ref"].as_str() == Some(self.run_ref.as_str()) && !run["end"].is_null()
        });
        let run = match carried {
            Some(run) => run.clone(),
            None => match self.stands(2) {
                Stands::Ended(run) => run,
                Stands::Running | Stands::NotRead => {
                    self.armed = false;
                    return self.not_read();
                }
            },
        };
        self.as_ended(run)
    }

    /// The report of a run that stands ended, and the guard disarmed.
    fn as_ended(&mut self, run: Value) -> Value {
        self.armed = false;
        json!({"run": run})
    }

    /// The report of a run that stands ended when how it ended was not read.
    fn not_read(&self) -> Value {
        json!({"run": {"run_ref": self.run_ref, "day_ref": self.day_ref,
            "state": ENDED_NOT_READ, "end": null}})
    }

    /// How this run stands at Central now, the Day's moves read again up to
    /// `tries` times while the read fails.
    fn stands(&self, tries: usize) -> Stands {
        for _ in 0..tries {
            let Ok(moves) = self
                .central
                .call("central.day.moves.read", &json!({"day_ref": self.day_ref}))
            else {
                continue;
            };
            let ours = moves["moves"][self.mv.name()]["runs"]
                .as_array()
                .and_then(|runs| {
                    runs.iter()
                        .find(|run| run["run_ref"].as_str() == Some(self.run_ref.as_str()))
                });
            return match ours {
                Some(run) if !run["end"].is_null() => Stands::Ended(run.clone()),
                _ => Stands::Running,
            };
        }
        Stands::NotRead
    }

    fn record_end(
        &mut self,
        state: &str,
        gave: Option<&Gave>,
        reason: Option<&str>,
    ) -> std::result::Result<Value, CentralError> {
        let mut input = json!({"event": "end", "run_ref": self.run_ref,
            "at_unix_ms": now_ms().max(self.begun_at), "state": state});
        let mut own = None;
        if let Some(gave) = gave {
            input["gave"] = gave.json();
            own = gave.says();
        }
        if let Some(reason) = reason {
            input["reason"] = json!(reason);
        }
        plain_record(&mut input, own);
        let ended = self.central.call("central.day.move.record", &input);
        if ended.is_ok() {
            self.armed = false;
        }
        ended
    }
}

// ---- what a run records is plain text ------------------------------------------

/// The characters that draw nothing, so text holding them reads other than it
/// is (be2j ruling 19; DLW build ruling 2026-11-10). The set is Unicode's own,
/// derived as Central derives it (`ctrl/src/names.rs` `invisible`) and as O-I
/// does (`desktop/cradle/src/shared/plainText.ts`): every format character
/// (Cf) save those that draw a sign, the line and paragraph separators, the
/// whole tag block and the letters that draw as blank space. With the
/// controls it is the list all three keep byte for byte under one sha256
/// (`invisible-code-points.txt`, beside this file).
fn invisible(c: char) -> bool {
    static FORMAT: OnceLock<Regex> = OnceLock::new();
    match c {
        '\u{2028}' | '\u{2029}' => true,
        '\u{E0000}'..='\u{E007F}' => true,
        '\u{034F}' | '\u{115F}' | '\u{1160}' | '\u{2800}' | '\u{3164}' | '\u{FFA0}' => true,
        '\u{0600}'..='\u{0605}'
        | '\u{06DD}'
        | '\u{0890}'
        | '\u{0891}'
        | '\u{08E2}'
        | '\u{110BD}'
        | '\u{110CD}' => false,
        c => FORMAT
            .get_or_init(|| Regex::new(r"^\p{Cf}$").expect("the format characters' class"))
            .is_match(c.encode_utf8(&mut [0; 4])),
    }
}

/// Where the escape that the ESC at `at` opens ends: the place to read on
/// from. The rules are Central's own for a notification's words
/// (`ctrl/src/notification.rs` `escape_end`, as be2j.c3 leaves it), so both
/// sides read one text alike (the cases both hold are `escape-cases.json`,
/// beside this file), and where a sequence is not whole the words a person
/// would read stay (be2j.a2 item 4; be2j.a3 item 5):
/// - a CSI in its strict form goes whole: ESC `[`, parameter characters
///   (U+0030–003F), then intermediate ones (U+0020–002F), then one final
///   character (U+0040–007E). Anything else after ESC `[` is not a CSI, and
///   only the ESC and the `[` go;
/// - a string sequence (OSC ESC `]`, DCS ESC `P`, APC ESC `_`, PM ESC `^`,
///   SOS ESC `X`) goes whole when it is ended on its own line: by ESC `\`, by
///   a BEL for an OSC, or by another ESC, which is then read as an opening of
///   its own. Its line ends where the cleaned words break: at a \n, a \r, a
///   vertical tab, a form feed, a next line, a line or paragraph separator.
///   Without an end only its two opening characters go;
/// - ESC, any intermediate characters (U+0020–002F) and one final character
///   (U+0030–007E) goes whole: with none between, a two-character escape
///   (ESC `7`, ESC `c`, ESC `=`), its letter with it; with some, a character
///   set selection and the like;
/// - any other ESC goes alone.
fn escape_end(chars: &[char], at: usize) -> usize {
    let run = |from: usize, low: char, high: char| {
        from + chars[from.min(chars.len())..]
            .iter()
            .take_while(|c| (low..=high).contains(*c))
            .count()
    };
    let is =
        |at: usize, low: char, high: char| chars.get(at).is_some_and(|c| (low..=high).contains(c));
    match chars.get(at + 1) {
        Some('[') => {
            let end = run(run(at + 2, '\u{30}', '\u{3f}'), '\u{20}', '\u{2f}');
            if is(end, '\u{40}', '\u{7e}') {
                end + 1
            } else {
                at + 2
            }
        }
        Some(&form @ (']' | 'P' | '_' | '^' | 'X')) => {
            for (end, &c) in chars.iter().enumerate().skip(at + 2) {
                match c {
                    '\u{07}' if form == ']' => return end + 1,
                    '\u{1b}' if chars.get(end + 1) == Some(&'\\') => return end + 2,
                    '\u{1b}' => return end,
                    '\n' | '\r' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => break,
                    _ => {}
                }
            }
            at + 2
        }
        _ => {
            let end = run(at + 1, '\u{20}', '\u{2f}');
            if is(end, '\u{30}', '\u{7e}') {
                end + 1
            } else {
                at + 1
            }
        }
    }
}

/// Text with each terminal escape left out, as a harness's coloured failure
/// reads as its words (`escape_end`). Only an ESC opens one: U+009B and U+009D
/// alone open nothing, since text decoded from the wrong character set
/// carries them, and they are controls for the caller to clean like any
/// other.
fn without_escapes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut plain = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        if chars[at] == '\u{1b}' {
            at = escape_end(&chars, at);
        } else {
            plain.push(chars[at]);
            at += 1;
        }
    }
    plain
}

/// Text in a block, as Central keeps a run's `reason` and `gave.summary`: its
/// terminal escapes left out (`without_escapes`), its line breaks and tabs
/// kept (a `\r` only before a `\n`), every other control a space, and every
/// other invisible character left out. What parts lines is a line break, as
/// Central's cleaning has it (be2j.c3 item 3): a line or paragraph separator,
/// a vertical tab, a form feed, a next line, and a `\r` that no `\n` follows.
fn block_plain(text: &str) -> String {
    let text = without_escapes(text);
    let mut plain = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' | '\t' => plain.push(c),
            '\r' if chars.peek() == Some(&'\n') => plain.push(c),
            '\r' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => plain.push('\n'),
            c if c.is_control() => plain.push(' '),
            c if invisible(c) => {}
            c => plain.push(c),
        }
    }
    plain
}

/// Central's bounds on a run's end, in bytes and rows, each with where Central
/// asks it (`ctrl/src/continuous_work/` at Central 9712b020). A record over
/// one is refused whole, so the runner keeps its record inside them before it
/// sends it (be2j.a3 item 1):
/// - the `reason`: `day_moves.rs` line 1127, `!reason.trim().is_empty() &&
///   reason.len() <= 4096`;
const CENTRAL_REASON_BYTES: usize = 4096;
/// - `gave.summary`: `source.rs` line 28 (`text`, "non-empty text of at most
///   4096 bytes"), asked at `day_moves.rs` line 1183;
const CENTRAL_SUMMARY_BYTES: usize = 4096;
/// - a ref in `gave.proposals` or `gave.writes`: `day_moves.rs` line 995,
///   `row.len() <= 4096`;
const CENTRAL_REF_BYTES: usize = 4096;
/// - the rows of each of those lists: `day_moves.rs` line 990, `rows.len() <=
///   256`;
const CENTRAL_REF_ROWS: usize = 256;
/// - `gave.points_at`: `day_moves.rs` line 1171, `points_at.len() <= 512`.
const CENTRAL_POINTS_AT_BYTES: usize = 512;

/// - `gave.counts`, as JSON: `day_moves.rs` line 1255 at Central a4f67f86,
///   `serde_json::to_string(counts)?.len() <= 16 * 1024`.
const CENTRAL_COUNTS_BYTES: usize = 16 * 1024;

/// What ends words cut to one of Central's bounds. It counts inside the bound.
const CUT_SHORT: &str = " …";

/// Words within `bound` bytes. Words over it keep their first part: they are
/// cut on a character boundary and end `CUT_SHORT`, the whole inside the
/// bound, so Central keeps the real words and does not refuse the record over
/// their length (be2j.a2 item 2).
fn within_bound(words: String, bound: usize) -> String {
    if words.len() <= bound {
        return words;
    }
    let mut end = bound.saturating_sub(CUT_SHORT.len());
    while !words.is_char_boundary(end) {
        end -= 1;
    }
    // A cut may fall after a `\r` whose `\n` is gone, and Central keeps no
    // `\r` alone: the words end where they end.
    format!("{}{CUT_SHORT}", words[..end].trim_end())
}

/// A run's counts inside Central's bound, and whether anything was cut to get
/// them there. The counts are numbers and a few short words; Central's own
/// words can be among them (a Close's `carry_reason`). Over the bound, the
/// longest words are cut to half their length, again until the whole fits;
/// with no words left to cut the counts go, so the record is never refused
/// over them (be2j.a4 item 3).
fn counts_within_bound(counts: &mut Map<String, Value>) -> bool {
    let size = |counts: &Map<String, Value>| {
        serde_json::to_string(counts).map_or(usize::MAX, |json| json.len())
    };
    let mut cut = false;
    while size(counts) > CENTRAL_COUNTS_BYTES {
        cut = true;
        let longest = counts
            .iter()
            .filter_map(|(key, value)| value.as_str().map(|words| (words.len(), key.clone())))
            .max();
        match longest {
            Some((len, key)) if len > 2 * CUT_SHORT.len() => {
                let words = counts[&key].as_str().unwrap_or_default().to_owned();
                counts.insert(key, json!(within_bound(words, len / 2)));
            }
            _ => counts.clear(),
        }
    }
    cut
}

/// Whether a ref a run gave is recorded, as it was given. One that is empty,
/// is over Central's bound, or holds a control or an invisible character is
/// left out: cleaned or cut, it would name something else (be2j.a2 item 5;
/// be2j.a3 item 1).
fn ref_kept(given: &str) -> bool {
    !given.trim().is_empty()
        && given.len() <= CENTRAL_REF_BYTES
        && !given.chars().any(|c| c.is_control() || invisible(c))
}

/// The end of a run as it is recorded. The agent's words reach the record
/// here, and Central refuses a record that holds a character of the set or is
/// over one of its bounds, so:
/// - its `reason` and `gave.summary` are cleaned in a block (`block_plain`).
///   The reason is trimmed, so one that is white space up to the bound keeps
///   its words (be2j.a3 item 3); left with nothing visible it says so; and
///   each is cut to Central's bound (`within_bound`);
/// - each ref in `gave.proposals` and `gave.writes`, and `gave.points_at`, is
///   recorded as given or left out (`ref_kept`; `points_at` holds no white
///   space either and has its own bound, as Central keeps it), never cleaned
///   into another ref. The summary then says how many were left out;
/// - a list longer than Central keeps to a record lists its first rows, and
///   the summary says how many of how many (be2j.a3 item 1). What the run
///   counts as proposed is unchanged: the count is the run's, the list is
///   what one record holds.
///
/// - `gave.counts` over Central's bound has its longest words cut until it
///   fits (`counts_within_bound`), and the summary says so (be2j.a4 item 3).
///
/// What the summary says of the record stands after the run's own words, and
/// it is the run's words that are cut to make room for it: every sentence the
/// record says of itself is whole, `own` first (what the run's own count says
/// of proposals Central named no ref for), then what was left out, then what
/// was listed short (be2j.a4 item 3).
fn plain_record(input: &mut Value, own: Option<String>) {
    if let Some(reason) = input.get("reason").and_then(Value::as_str) {
        let plain = block_plain(reason);
        let plain = plain.trim();
        input["reason"] = json!(if plain.is_empty() {
            "The run's reason held nothing visible.".to_owned()
        } else {
            within_bound(plain.to_owned(), CENTRAL_REASON_BYTES)
        });
    }
    let Some(gave) = input.get_mut("gave").and_then(Value::as_object_mut) else {
        return;
    };
    let mut left_out = 0;
    let mut said: Vec<String> = Vec::new();
    for (key, one, many) in [
        ("proposals", "proposal", "proposals"),
        ("writes", "write", "writes"),
    ] {
        if let Some(rows) = gave.get(key).and_then(Value::as_array) {
            let mut kept: Vec<&str> = rows
                .iter()
                .filter_map(Value::as_str)
                .filter(|row| ref_kept(row))
                .collect();
            left_out += rows.len() - kept.len();
            if kept.len() > CENTRAL_REF_ROWS {
                said.push(format!(
                    "This record lists the first {CENTRAL_REF_ROWS} of {}: Central keeps {CENTRAL_REF_ROWS} to a record.",
                    count_words(kept.len(), one, many)
                ));
                kept.truncate(CENTRAL_REF_ROWS);
            }
            let kept = json!(kept);
            gave.insert(key.into(), kept);
        }
    }
    if let Some(points_at) = gave.get("points_at").filter(|given| !given.is_null()) {
        let kept = points_at.as_str().is_some_and(|given| {
            ref_kept(given)
                && given.len() <= CENTRAL_POINTS_AT_BYTES
                && !given.chars().any(char::is_whitespace)
        });
        if !kept {
            gave.remove("points_at");
            left_out += 1;
        }
    }
    if gave
        .get_mut("counts")
        .and_then(Value::as_object_mut)
        .is_some_and(counts_within_bound)
    {
        said.push(format!(
            "This record's counts were cut to the {} KiB Central keeps of them.",
            CENTRAL_COUNTS_BYTES / 1024
        ));
    }
    let summary = gave.get("summary").and_then(Value::as_str).map(block_plain);
    if summary.is_none() && left_out == 0 && said.is_empty() && own.is_none() {
        return;
    }
    if left_out > 0 {
        said.insert(
            0,
            format!(
                "{} left out of this record: Central keeps no ref that is empty, over its length, or holds a character that does not show or white space where none may stand.",
                if left_out == 1 {
                    "1 ref was".to_owned()
                } else {
                    format!("{left_out} refs were")
                }
            ),
        );
    }
    if let Some(own) = own {
        said.insert(0, own);
    }
    let said = said.join(" ");
    let own = summary.unwrap_or_default();
    let summary = if said.is_empty() {
        within_bound(own, CENTRAL_SUMMARY_BYTES)
    } else if own.trim().is_empty() {
        said
    } else {
        let room = CENTRAL_SUMMARY_BYTES.saturating_sub(said.len() + 1);
        format!("{} {said}", within_bound(own.trim_end().to_owned(), room))
    };
    gave.insert("summary".into(), json!(summary));
}

impl Drop for OpenRun<'_> {
    fn drop(&mut self) {
        const WORDS: &str = "The runner stopped before it recorded how this run ended.";
        if !self.armed {
            return;
        }
        if self.end_tried {
            // The run's own end was refused or never answered: one try, no more.
            let _ = self.record_end("stalled", None, Some(WORDS));
        } else {
            let _ = self.end("stalled", None, Some(WORDS));
        }
    }
}

fn report(request: &RunRequest, outcome: &str, run: &Value, created: bool) -> Value {
    json!({
        "schema": SCHEMA,
        "move": request.mv.name(),
        "outcome": outcome,
        "created": created,
        "trigger": request.trigger.name(),
        "occurrence_ref": request.occurrence_ref,
        "day_ref": run["day_ref"],
        "run_ref": run["run_ref"],
        "state": run["state"],
        "agent_writes": run["agent_writes"],
        "reason": run["end"]["reason"],
        "gave": run["end"]["gave"],
        "proposals": run["end"]["gave"]["proposals"],
        "run": run,
    })
}

/// Run one move once. An `Err` before the run begins means nothing was
/// recorded. An `Err` after it means the run was not known ended at Central
/// when the runner last looked: Central could not be reached, or refused the
/// `refused` end as well while the run still ran or could not be read. The guard's `stalled` end,
/// tried once as the run is dropped, may still have been recorded, so the run
/// may read `stalled` in Central. Every other begun run comes back as a report
/// with the end it stands at: its own, the end another gave it when the run
/// was ended elsewhere first, or the state `ENDED_NOT_READ` when Central says
/// it has ended and how could not be read (be2j.a3 item 2).
///
/// How long the end can take (be2j.a4 item 4). Each call to Central waits at
/// most 120 seconds (`CtrlCentral::call`). After the agent's turn, the most
/// calls that can go unanswered before `run_move` returns is seven: two
/// conflicts each followed by two lost reads, the `refused` end lost, one
/// read, and the guard's `stalled`. That is 840 seconds, and it needs `ctrl`
/// to answer two ends and then hang seven times. It is left at that: the
/// agent's turn before it waits up to `--timeout-secs` (600 by default) plus
/// 60, so the end's worst case is of the run's own order, and the only caller
/// is `aikit work day-move run` (and `due`, which runs each due move in
/// turn), a command a person or a host's schedule starts; nothing in AIKit,
/// O-I or Factory calls it with a timeout of its own to outlast.
pub fn run_move(
    central: &dyn CentralPort,
    agent: &dyn DayAgent,
    request: &RunRequest,
) -> Result<Value> {
    if request.trigger == Trigger::Schedule && request.occurrence_ref.is_none() {
        return Err(fail(
            "day_move.occurrence_required",
            "a scheduled run names the occurrence it runs (--occurrence-ref)",
        ));
    }
    if request.trigger == Trigger::Press && request.occurrence_ref.is_some() {
        return Err(fail(
            "day_move.occurrence_unexpected",
            "a press has no occurrence; only a schedule does",
        ));
    }
    let mv = request.mv;
    let mut read_input = json!({});
    if let Some(day_ref) = &request.day_ref {
        read_input["day_ref"] = json!(day_ref);
    }
    let moves = central
        .call("central.day.moves.read", &read_input)
        .map_err(central_failure)?;
    let day_ref = moves["day_ref"]
        .as_str()
        .ok_or_else(|| {
            fail(
                "day_move.central_invalid",
                "central.day.moves.read named no Day",
            )
        })?
        .to_owned();
    let world_ref = moves["settings"]["scope_ref"]
        .as_str()
        .unwrap_or("control:root")
        .to_owned();
    let begun_at = now_ms();
    let mut begin = json!({"event": "begin", "move": mv.name(), "at_unix_ms": begun_at,
        "trigger": request.trigger.name(), "day_ref": day_ref});
    if let Some(occurrence) = &request.occurrence_ref {
        begin["occurrence_ref"] = json!(occurrence);
    }
    let began = central
        .call("central.day.move.record", &begin)
        .map_err(central_failure)?;
    let run = began["run"].clone();
    let created = began["created"].as_bool().unwrap_or(false);
    let run_ref = run["run_ref"]
        .as_str()
        .ok_or_else(|| fail("day_move.central_invalid", "the begun run has no run_ref"))?
        .to_owned();
    if !created && run["state"] != "running" {
        // The occurrence ran already: it is not run again.
        return Ok(report(request, "skipped", &run, false));
    }
    let mut open = OpenRun {
        central,
        mv,
        day_ref: day_ref.clone(),
        run_ref: run_ref.clone(),
        begun_at: run["begun_at_unix_ms"].as_i64().unwrap_or(begun_at),
        armed: true,
        end_tried: false,
    };
    let context = RunContext {
        mv,
        day_ref,
        world_ref,
        run_ref,
        prompt: run["prompt"].as_str().unwrap_or_default().to_owned(),
        agent_writes: run["agent_writes"].as_str().unwrap_or("propose").to_owned(),
    };
    let ended = match execute(central, agent, &context) {
        Ok(gave) => open.end("done", Some(&gave), None),
        Err(stop) => open.end(stop.state, stop.gave.as_ref(), Some(&stop.reason)),
    }?;
    Ok(report(
        request,
        if created { "ran" } else { "resumed" },
        &ended["run"],
        created,
    ))
}

/// Run each occurrence a saved schedule made due in `(since, now]` that has not
/// begun, once, in time order. An occurrence that began (running or ended) is
/// skipped. Nothing calls this by itself.
pub fn run_due(
    central: &dyn CentralPort,
    agent: &dyn DayAgent,
    since_unix_ms: i64,
    now_unix_ms: i64,
) -> Result<Value> {
    let due = central
        .call(
            "central.day.moves.due",
            &json!({"since_unix_ms": since_unix_ms, "now_unix_ms": now_unix_ms}),
        )
        .map_err(central_failure)?;
    let mut rows: Vec<Value> = due["due"].as_array().cloned().unwrap_or_default();
    rows.sort_by_key(|row| row["due_unix_ms"].as_i64().unwrap_or(0));
    let mut outcomes = Vec::new();
    for row in &rows {
        let occurrence = row["occurrence_ref"].as_str().unwrap_or_default();
        if row["begun"] == true {
            outcomes.push(json!({"outcome": "skipped", "move": row["move"],
                "occurrence_ref": occurrence, "run_ref": row["run_ref"],
                "reason": "this occurrence has begun already; it is not run again"}));
            continue;
        }
        let request = match Move::parse(row["move"].as_str().unwrap_or_default()) {
            Ok(mv) => RunRequest {
                mv,
                day_ref: row["day_ref"].as_str().map(str::to_owned),
                trigger: Trigger::Schedule,
                occurrence_ref: Some(occurrence.to_owned()),
            },
            Err(error) => {
                outcomes.push(json!({"outcome": "not-run", "move": row["move"],
                    "occurrence_ref": occurrence, "reason": error.message()}));
                continue;
            }
        };
        match run_move(central, agent, &request) {
            Ok(report) => outcomes.push(report),
            Err(error) => outcomes.push(json!({"outcome": "not-run", "move": row["move"],
                "occurrence_ref": occurrence, "reason": error.message()})),
        }
    }
    Ok(json!({
        "schema": "aikit.day-move-due/v1",
        "since_unix_ms": since_unix_ms,
        "now_unix_ms": now_unix_ms,
        "standing": due["standing"],
        "reason": due["reason"],
        "occurrences": rows.len(),
        "outcomes": outcomes,
    }))
}

// ---- the command surface -------------------------------------------------------------

/// What `aikit work day-move` needs besides the move.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub central_root: Option<PathBuf>,
    pub ctrl_bin: Option<PathBuf>,
    pub project: Option<String>,
    pub provider: Option<String>,
    pub agent_ref: Option<String>,
    pub timeout_secs: Option<u64>,
}

fn central_for(settings: &Settings) -> Result<CtrlCentral> {
    let root = match &settings.central_root {
        Some(root) => root.clone(),
        None => crate::routine_dispatch::CtrlOccurrenceSource::discover()?,
    };
    let ctrl = settings
        .ctrl_bin
        .clone()
        .or_else(|| std::env::var_os("CENTRAL_CTRL_BIN").map(PathBuf::from))
        .or_else(|| std::env::var_os("OI_CENTRAL_CTRL_BIN").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("ctrl"));
    Ok(CtrlCentral {
        ctrl,
        root,
        project: settings.project.clone(),
        token: None,
    })
}

fn resident_for(home: &AikitHome, central: &CtrlCentral, settings: &Settings) -> ResidentDayAgent {
    ResidentDayAgent {
        home: home.clone(),
        cwd: std::fs::canonicalize(&central.root).unwrap_or_else(|_| central.root.clone()),
        provider: settings.provider.clone(),
        agent_ref: settings.agent_ref.clone(),
        timeout: Duration::from_secs(settings.timeout_secs.unwrap_or(600)),
    }
}

/// `aikit work day-move run`.
pub fn run(home: &AikitHome, settings: &Settings, request: &RunRequest) -> Result<Value> {
    let central = central_for(settings)?;
    let agent = resident_for(home, &central, settings);
    run_move(&central, &agent, request)
}

/// `aikit work day-move due`.
pub fn due(
    home: &AikitHome,
    settings: &Settings,
    since_unix_ms: i64,
    now_unix_ms: i64,
) -> Result<Value> {
    let central = central_for(settings)?;
    let agent = resident_for(home, &central, settings);
    run_due(&central, &agent, since_unix_ms, now_unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_is_one_of_five() {
        assert_eq!(Move::parse("close").unwrap(), Move::Close);
        assert!(Move::parse("rollover").is_err());
        assert_eq!(Move::ALL.len(), 5);
    }

    #[test]
    fn tasks_answer_is_strict() {
        let ok = parse_answer(
            Move::Tasks,
            r#"{"must":["ring the printer"],"should":[],"could":["tidy"]}"#,
        )
        .unwrap();
        assert_eq!(ok.entries.len(), 2);
        assert_eq!(ok.entries[0].kind, "must");
        assert_eq!(ok.entries[1].kind, "could");
        for bad in [
            "Here are your tasks: …",
            r#"{"must":[],"should":[]}"#,
            r#"{"must":[],"should":[],"could":[],"wont":[]}"#,
            r#"{"must":[3],"should":[],"could":[]}"#,
            r#"{"must":[""],"should":[],"could":[]}"#,
            r#"[]"#,
        ] {
            assert!(parse_answer(Move::Tasks, bad).is_err(), "{bad}");
        }
        // A fence around the one document is formatting; prose around it is not.
        assert!(parse_answer(
            Move::Tasks,
            "```json\n{\"must\":[],\"should\":[],\"could\":[]}\n```"
        )
        .is_ok());
        assert!(parse_answer(
            Move::Tasks,
            "Sure!\n{\"must\":[],\"should\":[],\"could\":[]}"
        )
        .is_err());
    }

    #[test]
    fn patterns_context_synthesis_and_close_answers_are_strict() {
        let patterns = parse_answer(
            Move::Patterns,
            r#"[{"pattern":"reading before writing","items":["a","b"]}]"#,
        )
        .unwrap();
        assert!(patterns.entries[0].html.contains("<li>a</li><li>b</li>"));
        assert!(parse_answer(Move::Patterns, r#"[{"pattern":"x","items":[]}]"#).is_err());
        assert!(parse_answer(Move::Patterns, r#"[{"pattern":"x"}]"#).is_err());

        let context = parse_answer(
            Move::Context,
            r#"[{"kind":"file","text":"a.rs"},{"kind":"person","text":"Ana","ref":"p"},{"kind":"moment","text":"noon"},{"kind":"place","text":"the harbour"}]"#,
        )
        .unwrap();
        let kinds: Vec<_> = context.entries.iter().map(|entry| entry.kind).collect();
        assert_eq!(kinds, ["file", "person", "moment", "place"]);
        assert!(parse_answer(Move::Context, r#"[{"kind":"thing","text":"x"}]"#).is_err());
        assert!(parse_answer(Move::Context, r#"[{"text":"x"}]"#).is_err());

        let synthesis = parse_answer(
            Move::Synthesis,
            r#"{"taught":"a","open":"","tomorrow":"c"}"#,
        )
        .unwrap();
        assert_eq!(synthesis.entries.len(), 2, "an empty part proposes nothing");
        assert!(parse_answer(Move::Synthesis, r#"{"taught":"a"}"#).is_err());

        let close = parse_answer(Move::Close, r#"{"summary":"s","learned":["x","y"]}"#).unwrap();
        assert_eq!(close.entries.len(), 1);
        assert_eq!(close.learned, 2);
        assert!(parse_answer(Move::Close, r#"{"summary":"","learned":[]}"#).is_err());
    }

    #[test]
    fn entries_are_escaped_into_the_page() {
        let answer = parse_answer(
            Move::Tasks,
            r#"{"must":["<script>x</script> & more"],"should":[],"could":[]}"#,
        )
        .unwrap();
        assert_eq!(
            answer.entries[0].html,
            "<p>&lt;script&gt;x&lt;/script&gt; &amp; more</p>"
        );
    }

    #[test]
    fn a_proposal_id_is_the_run_the_field_and_the_entry() {
        let a = proposal_key("run:1", "p1_must", "<p>a</p>");
        assert_eq!(a, proposal_key("run:1", "p1_must", "<p>a</p>"));
        assert_ne!(a, proposal_key("run:2", "p1_must", "<p>a</p>"));
        assert_ne!(a, proposal_key("run:1", "p1_should", "<p>a</p>"));
        assert_ne!(a, proposal_key("run:1", "p1_must", "<p>b</p>"));
    }

    fn document(pointers: &[(&str, &str)]) -> DayDocument {
        DayDocument {
            source_ref: "s".into(),
            document_id: "d".into(),
            revision: "r".into(),
            fields: pointers
                .iter()
                .map(|(id, pointer)| ((*id).into(), (*pointer).into()))
                .collect(),
            payload: json!({"fields": {"p0_quick_thoughts": "<p>Ring the <b>printer</b></p>"}, "body": {}}),
            contributions: vec![],
        }
    }

    #[test]
    fn an_entry_lands_only_on_a_field_the_document_maps() {
        let answer = parse_answer(
            Move::Synthesis,
            r#"{"taught":"t","open":"o","tomorrow":"m"}"#,
        )
        .unwrap();
        // Only face 5's synthesis is mapped: open and tomorrow fall back to it,
        // each saying what it is.
        let (resolved, unmapped) = resolve(
            &answer.entries,
            &document(&[("native:p5_synthesis", "/fields/p5_synthesis")]),
        );
        assert!(unmapped.is_empty());
        assert_eq!(resolved.len(), 3);
        assert!(resolved.iter().all(|r| r.field_id == "native:p5_synthesis"));
        assert!(resolved[1]
            .html
            .starts_with("<p><strong>Still open</strong></p>"));
        assert!(resolved[2]
            .html
            .starts_with("<p><strong>For tomorrow</strong></p>"));
        assert!(!resolved[0].html.contains("<strong>"));
        // Their own fields are used when the document maps them.
        let (resolved, _) = resolve(
            &answer.entries,
            &document(&[
                ("p5_synthesis", "/fields/p5_synthesis"),
                ("blockers", "/body/p5_blockers"),
                ("tomorrow", "/body/p5_tomorrow"),
            ]),
        );
        let fields: Vec<_> = resolved.iter().map(|r| r.field_id.as_str()).collect();
        assert_eq!(fields, ["p5_synthesis", "blockers", "tomorrow"]);
        assert!(!resolved[1].html.contains("Still open"));
        // A moment with no face-4 field is not put on another face.
        let moment = parse_answer(Move::Context, r#"[{"kind":"moment","text":"noon"}]"#).unwrap();
        let (resolved, unmapped) = resolve(
            &moment.entries,
            &document(&[("f", "/fields/p4_files_touched")]),
        );
        assert!(resolved.is_empty());
        assert_eq!(unmapped.len(), 1);
    }

    #[test]
    fn the_faces_text_is_what_the_page_holds() {
        let text = face_text(&document(&[("g", "/fields/p0_quick_thoughts")]), 0);
        assert!(text.starts_with("Face 0, Ground"), "{text}");
        assert!(
            text.contains("p0_quick_thoughts: Ring the printer"),
            "{text}"
        );
        assert!(!text.contains("p0_adjacencies"), "{text}");
        let empty = face_text(&document(&[]), 3);
        assert!(empty.contains("nothing written on this face"), "{empty}");
    }

    #[test]
    fn a_day_session_is_found_by_its_world() {
        let (space, session) = day_session("control:root");
        assert_eq!(
            (space.clone(), session.clone()),
            day_session("control:root")
        );
        assert_ne!(session, day_session("project:Alpha").1);
        assert!(space.to_string().starts_with("session-space/the-day-"));
        assert!(session.as_str().starts_with("agent-session/the-day-"));
    }

    #[test]
    fn plain_text_keeps_block_ends_and_drops_tags() {
        assert_eq!(
            plain_text("<p>a &amp; b</p><ul><li>c</li><li>d</li></ul>"),
            "a & b\nc\nd"
        );
    }

    // ---- be2j ruling 19: a run's record is plain text, and a begun run is never left unended ----

    /// The words Central gives when it refuses a run's end (`day_moves.rs` move_record).
    const CENTRAL_WORDS: &str = "gave.summary holds no control character but line breaks and tabs (a \\r only before a \\n), and no invisible one (a line or paragraph separator, a format character or a blank filler)";

    /// Central's refusal codes as the runner meets them (`action_result` in
    /// `ctrl/src/continuous_work/mod.rs` at Central 63712618): a request it
    /// finds invalid, and a record that met a newer state than it was made
    /// against. Written out here, so a change of the runner's own constant is
    /// a change these tests see.
    const INVALID: &str = "invalid_native_source_or_request";
    const CONFLICT: &str = "stale_basis_or_identity_conflict";
    /// Central's code of its own for a run that has ended already (be2j.c3).
    const RUN_ENDED: &str = "run_ended_already";

    /// How the test's Central names a run that has ended already.
    #[derive(Clone, Copy, Default, PartialEq)]
    enum Names {
        /// As a Central before be2j.c3: the conflict's code.
        #[default]
        Conflict,
        /// By the code of its own, with the run as it ended in the details.
        EndedWithTheRun,
        /// By the code of its own, with no run in the details.
        EndedBare,
        /// By the code of its own, with another run in the details: an earlier
        /// run of the move, which ended done.
        EndedWithAnotherRun,
        /// By the code of its own, with this run in the details as it stood
        /// while it ran: no end.
        EndedWithTheRunRunning,
    }

    /// What the test's Central answers one end with, in place of checking it.
    #[derive(Clone)]
    enum Answer {
        /// Refused as invalid, in these words.
        Invalid(String),
        /// Refused as a conflict.
        Conflict,
        /// Central could not be reached.
        Unavailable,
    }

    /// Central as the runner meets it, kept in memory: a Day whose document
    /// maps face 1's must, and every end the runner records, kept as it was
    /// given. An end is answered from `script`, then by `rest`; with neither
    /// it is checked as Central checks it (`central_end`) and recorded or
    /// refused in Central's words. The Day's moves always hold another run of
    /// the move that has ended, and this run still running unless
    /// `ended_elsewhere`.
    #[derive(Default)]
    struct Recording {
        ends: std::sync::Mutex<Vec<Value>>,
        script: std::sync::Mutex<std::collections::VecDeque<Answer>>,
        rest: Option<Answer>,
        /// The run was stopped from elsewhere: every end is a conflict, and the
        /// Day's moves show the run ended.
        ended_elsewhere: bool,
        moves_reads: std::sync::atomic::AtomicUsize,
        /// What a submit is answered with, by its number; a numbered ref when none.
        names: Option<fn(usize) -> Value>,
        /// Central holds the proposal already: a submit is refused, and the
        /// read of what it holds answers this.
        held: Option<Value>,
        submitted: std::sync::atomic::AtomicUsize,
        /// How an end for an ended run is refused.
        ended_names: Names,
        /// The run is ended elsewhere once the Day's moves were read this many times.
        ended_from_read: Option<usize>,
        /// Which reads of the Day's moves fail, by their number from 0.
        reads_fail: Option<fn(usize) -> bool>,
        /// The first end is taken and its answer never comes.
        answer_lost: bool,
        /// The run as an end with a lost answer left it.
        taken: std::sync::Mutex<Option<Value>>,
        /// The submit of this number is refused.
        refused_submit: Option<usize>,
        /// What `central.day.close-carry` answers: "refused", or the carry's state.
        carry: Option<&'static str>,
        /// The run is ended elsewhere once this many ends were sent, this one
        /// counted: that end is refused as an end for an ended run.
        ended_after_ends: Option<usize>,
    }

    impl Recording {
        fn scripted(script: &[Answer]) -> Recording {
            Recording {
                script: std::sync::Mutex::new(script.iter().cloned().collect()),
                ..Recording::default()
            }
        }

        /// Each end the runner sent, in order: its state, and whether it carried
        /// what the run gave.
        fn states(&self) -> Vec<String> {
            self.ends
                .lock()
                .unwrap()
                .iter()
                .map(|end| {
                    format!(
                        "{}{}",
                        end["state"].as_str().unwrap_or("?"),
                        if end.get("gave").is_some() {
                            "+gave"
                        } else {
                            ""
                        }
                    )
                })
                .collect()
        }

        fn moves_reads(&self) -> usize {
            self.moves_reads.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn submitted(&self) -> usize {
            self.submitted.load(std::sync::atomic::Ordering::SeqCst)
        }

        /// The run as it stands ended, if it does: stopped from elsewhere, or
        /// by the end whose answer was lost.
        fn ended(&self) -> Option<Value> {
            if let Some(taken) = self.taken.lock().unwrap().clone() {
                return Some(taken);
            }
            let elsewhere = self.ended_elsewhere
                || self
                    .ended_from_read
                    .is_some_and(|reads| self.moves_reads() >= reads)
                || self
                    .ended_after_ends
                    .is_some_and(|ends| self.ends.lock().unwrap().len() >= ends);
            elsewhere.then(|| {
                json!({"run_ref": "run:1", "day_ref": "day:2026-11-10", "state": "stopped",
                    "agent_writes": "propose",
                    "end": {"state": "stopped", "reason": "Stopped from the Day's page."}})
            })
        }
    }

    impl CentralPort for Recording {
        fn call(&self, action: &str, input: &Value) -> std::result::Result<Value, CentralError> {
            let refuse = |code: &str,
                          message: &str,
                          unavailable: bool|
             -> std::result::Result<Value, CentralError> {
                Err(CentralError {
                    code: code.into(),
                    message: message.into(),
                    unavailable,
                    details: Value::Null,
                })
            };
            match (action, input["event"].as_str()) {
                ("central.day.moves.read", _) => {
                    let ours = self.ended().unwrap_or_else(|| {
                        json!({"run_ref": "run:1", "day_ref": "day:2026-11-10", "state": "running",
                            "agent_writes": "propose", "end": null})
                    });
                    let number = self
                        .moves_reads
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if self.reads_fail.is_some_and(|fails| fails(number)) {
                        return refuse("day_move.central_unavailable", "ctrl did not answer", true);
                    }
                    Ok(json!({"day_ref": "day:2026-11-10",
                        "settings": {"scope_ref": "control:root"},
                        "moves": {"tasks": {"runs": [
                            {"run_ref": "run:0", "day_ref": "day:2026-11-10", "state": "done",
                                "agent_writes": "propose",
                                "end": {"state": "done", "gave": {"summary": "Tasks: an earlier run."}}},
                            ours]}}}))
                }
                ("central.day.move.record", Some("begin")) => Ok(json!({"created": true,
                    "run": {"run_ref": "run:1", "state": "running", "prompt": "Plan the day.",
                        "agent_writes": "propose", "begun_at_unix_ms": 1}})),
                ("central.day.move.record", Some("end")) => {
                    self.ends.lock().unwrap().push(input.clone());
                    // Central's words for a run that has ended are the words of every
                    // conflict here, also for a run that has not ended: what the
                    // runner does next goes by the run as it reads, never by words.
                    let ended_already = "run:1 has ended already";
                    if let Some(run) = self.ended() {
                        return match self.ended_names {
                            Names::Conflict => refuse(CONFLICT, ended_already, false),
                            named => Err(CentralError {
                                code: RUN_ENDED.into(),
                                message: ended_already.into(),
                                unavailable: false,
                                details: match named {
                                    Names::EndedWithTheRun => {
                                        json!({"run_ref": "run:1", "run": run})
                                    }
                                    Names::EndedWithAnotherRun => json!({"run_ref": "run:0",
                                        "run": {"run_ref": "run:0", "day_ref": "day:2026-11-10",
                                            "state": "done", "agent_writes": "propose",
                                            "end": {"state": "done", "gave": {"summary": "Tasks: an earlier run."}}}}),
                                    Names::EndedWithTheRunRunning => json!({"run_ref": "run:1",
                                        "run": {"run_ref": "run:1", "day_ref": "day:2026-11-10",
                                            "state": "running", "agent_writes": "propose", "end": null}}),
                                    _ => json!({}),
                                },
                            }),
                        };
                    }
                    if self.answer_lost {
                        *self.taken.lock().unwrap() = Some(json!({"run_ref": "run:1",
                            "day_ref": "day:2026-11-10", "state": input["state"], "agent_writes": "propose",
                            "end": {"state": input["state"], "reason": input["reason"], "gave": input["gave"]}}));
                        return refuse("day_move.central_unavailable", "ctrl did not answer", true);
                    }
                    let scripted = self.script.lock().unwrap().pop_front();
                    match scripted.or_else(|| self.rest.clone()) {
                        Some(Answer::Invalid(words)) => refuse(INVALID, &words, false),
                        Some(Answer::Conflict) => refuse(CONFLICT, ended_already, false),
                        Some(Answer::Unavailable) => {
                            refuse("day_move.central_unavailable", "ctrl did not answer", true)
                        }
                        None => match central_end(input, "tasks", "propose") {
                            Ok(()) => {
                                Ok(json!({"run": {"run_ref": "run:1", "state": input["state"],
                                "end": {"reason": input["reason"], "gave": input["gave"]}}}))
                            }
                            Err(words) => refuse(INVALID, &words, false),
                        },
                    }
                }
                ("central.receiving.submit", _) => {
                    let number = self
                        .submitted
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    if self.held.is_some() {
                        return refuse(CONFLICT, "this proposal is held already", false);
                    }
                    if self.refused_submit == Some(number) {
                        return refuse(INVALID, "Central keeps no such proposal", false);
                    }
                    Ok(match self.names {
                        Some(names) => names(number),
                        None => json!({"return_ref": format!("central:return:{number}")}),
                    })
                }
                ("central.receiving.read", _) if self.held.is_some() => {
                    Ok(self.held.clone().unwrap_or_default())
                }
                ("central.day.close-carry", _) => match self.carry {
                    Some("refused") => refuse(INVALID, "the Day is not ready to close", false),
                    Some("waits") => Ok(json!({"tomorrow": {"day_ref": "day:2026-11-11"},
                        "carry": {"state": "waits", "proposal": {"source_ref": "source:tomorrow",
                            "document_id": "day:tomorrow", "expected_source_revision": "t1",
                            "proposal": {"target": "/fields/p0_ground"}}}})),
                    Some(state) => Ok(json!({"carry": {"state": state}})),
                    None => refuse("test.unexpected", "the test's Central closes no Day", true),
                },
                ("central.day.read", _) => Ok(json!({"day_ref": "day:2026-11-10",
                    "temporal": {"civil_date": "2026-11-10", "lifecycle": "open"},
                    "revision": {"revision": "r1"}, "relations_revision": "rr1",
                    "document": {"source": {"ref": "source:day"}, "document_id": "day:doc",
                        "revision": {"revision": "d1"},
                        "document": {"fields": [{"id": "p1_must", "template_pointer": "/fields/p1_must"},
                                {"id": "p5_synthesis", "template_pointer": "/fields/p5_synthesis"}],
                            "template_payload": {"fields": {}, "body": {}}, "contributions": []}}})),
                _ => refuse(
                    "test.unexpected",
                    &format!("the test's Central does not answer {action}"),
                    true,
                ),
            }
        }
    }

    /// The Day's agent, saying one thing.
    struct Says(std::result::Result<String, TurnFailure>);

    impl DayAgent for Says {
        fn agent_ref(&self, _: &str) -> String {
            "agent:the-day:test".into()
        }
        fn session_ref(&self, _: &str) -> Option<String> {
            None
        }
        fn turn(&self, _: &str, _: &str) -> std::result::Result<String, TurnFailure> {
            self.0.clone()
        }
    }

    /// The Day's agent, its harness failing in these words.
    fn fails(words: &str) -> Says {
        Says(Err(TurnFailure::Failed(words.to_owned())))
    }

    /// The Day's agent, answering Tasks with these musts.
    fn proposes(musts: &[&str]) -> Says {
        Says(Ok(
            json!({"must": musts, "should": [], "could": []}).to_string()
        ))
    }

    fn press_tasks() -> RunRequest {
        RunRequest {
            mv: Move::Tasks,
            day_ref: Some("day:2026-11-10".into()),
            trigger: Trigger::Press,
            occurrence_ref: None,
        }
    }

    /// The guard's words for a run the runner left unended.
    const LEFT_UNENDED: &str = "The runner stopped before it recorded how this run ended.";

    #[test]
    fn an_agents_words_holding_an_override_are_recorded_cleaned() {
        // The agent's own words, as its harness gave them back, stall the run.
        let central = Recording::default();
        let agent = fails("the agent said: Ro\u{202E}sa\u{200B}rium is\u{0007}late\u{2028}again");
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        let plain = "the agent said: Rosarium is late\nagain";
        assert_eq!(report["reason"], plain, "{report}");
        assert_eq!(central.ends.lock().unwrap()[0]["reason"], plain);
        // An answer whose own key holds an override: the run says why it did not
        // parse, in the agent's words, cleaned.
        let central = Recording::default();
        let agent = Says(Ok("{\"must\u{202E}\":[],\"should\":[],\"could\":[]}".into()));
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        assert_eq!(
            report["reason"],
            "The agent's answer did not parse as Tasks's shape: it has a `must`, which the shape does not",
            "{report}"
        );
    }

    // ---- be2j.a2 item 3: what answers a refused end goes by the refusal's code ----

    #[test]
    fn a_record_central_refuses_ends_the_run_refused_in_centrals_words() {
        let central = Recording::scripted(&[Answer::Invalid(CENTRAL_WORDS.into())]);
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        let words = format!("Central refused the record of how this run ended: {CENTRAL_WORDS}");
        assert_eq!(report["state"], "refused", "{report}");
        assert_eq!(report["reason"], words.as_str(), "{report}");
        assert_eq!(
            central.states(),
            ["done+gave", "refused"],
            "the refused end, then the run ended refused, without what Central refused"
        );
        assert_eq!(central.ends.lock().unwrap()[1]["reason"], words.as_str());
        assert_eq!(
            central.moves_reads(),
            1,
            "a record refused for what it holds is not read again"
        );
    }

    #[test]
    fn a_conflict_while_the_run_still_runs_records_the_same_end_once_more() {
        let central = Recording::scripted(&[Answer::Conflict]);
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "done", "{report}");
        assert_eq!(report["proposals"], json!(["central:return:0"]), "{report}");
        assert_eq!(
            central.states(),
            ["done+gave", "done+gave"],
            "the Day changed under the first end, and the same end is recorded once more"
        );
        let ends = central.ends.lock().unwrap();
        assert_eq!(
            ends[0]["gave"], ends[1]["gave"],
            "what the run gave is given again, whole"
        );
        assert_eq!(ends[1]["gave"]["proposals"], json!(["central:return:0"]));
        assert!(ends[1].get("reason").is_none(), "{:?}", ends[1]);
        assert_eq!(
            central.moves_reads(),
            2,
            "read once to begin, and once to tell a changed Day from an ended run"
        );
        assert_eq!(central.submitted(), 1, "nothing is proposed again");
    }

    #[test]
    fn a_conflict_refused_again_ends_the_run_refused() {
        let central = Recording::scripted(&[Answer::Conflict, Answer::Conflict]);
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "refused", "{report}");
        assert_eq!(
            report["reason"],
            "Central refused the record of how this run ended: run:1 has ended already",
            "{report}"
        );
        assert_eq!(central.states(), ["done+gave", "done+gave", "refused"]);
        assert_eq!(
            central.moves_reads(),
            3,
            "the run is read after each conflict of its own end, since it may have ended between the two"
        );
        // A conflict every time, the run still running: the same end once more,
        // the refused end, and the guard's one try. Four, and no more.
        let central = Recording {
            rest: Some(Answer::Conflict),
            ..Recording::default()
        };
        let error = run_move(&central, &agent, &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(
            central.states(),
            ["done+gave", "done+gave", "refused", "stalled"]
        );
        assert_eq!(
            central.moves_reads(),
            4,
            "to begin, and after each of the three conflicts: it still runs, so the error stands"
        );
    }

    #[test]
    fn a_run_ended_elsewhere_stays_as_it_ended_and_no_further_end_is_sent() {
        let central = Recording {
            ended_elsewhere: true,
            ..Recording::default()
        };
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        // The report is the run as it ended: this run's own end, never the other
        // run of the move that the Day's moves also hold ended (`run:0`, done).
        assert_eq!(report["run_ref"], "run:1", "{report}");
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
        assert_eq!(
            central.states(),
            ["done+gave"],
            "one end, refused; the run is ended, so no other is sent, by the runner or by its guard"
        );
        assert_eq!(central.moves_reads(), 2);
    }

    #[test]
    fn central_out_of_reach_at_the_end_is_an_error_and_the_guard_tries_stalled_once() {
        // Out of reach once, then back: the run's end is not tried again, and the
        // guard's one try records the run stalled.
        let central = Recording::scripted(&[Answer::Unavailable]);
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_unavailable");
        assert_eq!(central.states(), ["done+gave", "stalled"]);
        assert_eq!(central.ends.lock().unwrap()[1]["reason"], LEFT_UNENDED);
        assert_eq!(
            central.moves_reads(),
            2,
            "read once after the answer that never came: the run still runs, so the end was not taken"
        );
        // Out of reach for good: the same two tries, and no more.
        let central = Recording {
            rest: Some(Answer::Unavailable),
            ..Recording::default()
        };
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_unavailable");
        assert_eq!(central.states(), ["done+gave", "stalled"]);
    }

    #[test]
    fn a_conflict_and_then_central_out_of_reach_leaves_the_guard_one_try() {
        let central = Recording::scripted(&[Answer::Conflict, Answer::Unavailable]);
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_unavailable");
        assert_eq!(
            central.states(),
            ["done+gave", "done+gave", "stalled"],
            "the same end once more, out of reach; then only the guard's stalled, never a refused end"
        );
        assert_eq!(central.ends.lock().unwrap()[2]["reason"], LEFT_UNENDED);
    }

    #[test]
    fn a_record_central_refuses_every_time_is_tried_three_times_and_no_more() {
        let central = Recording {
            rest: Some(Answer::Invalid("Central keeps no such record".into())),
            ..Recording::default()
        };
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(
            central.states(),
            ["done+gave", "refused", "stalled"],
            "the run's end, the refused end, and the guard's one try"
        );
        assert_eq!(central.moves_reads(), 1);
    }

    /// The Day's agent, its turn panicking.
    struct Panics;

    impl DayAgent for Panics {
        fn agent_ref(&self, _: &str) -> String {
            "agent:the-day:test".into()
        }
        fn session_ref(&self, _: &str) -> Option<String> {
            None
        }
        fn turn(&self, _: &str, _: &str) -> std::result::Result<String, TurnFailure> {
            panic!("the agent's turn panicked (this test's own panic)")
        }
    }

    #[test]
    fn a_run_the_runner_left_unended_is_ended_stalled_by_its_guard_through_a_conflict() {
        // No end of the run's own was tried, so the guard's end is answered as
        // any end is: the Day changed under it, and it is recorded once more.
        let central = Recording::scripted(&[Answer::Conflict]);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = run_move(&central, &Panics, &press_tasks());
        }));
        assert!(panicked.is_err());
        assert_eq!(central.states(), ["stalled", "stalled"]);
        let ends = central.ends.lock().unwrap();
        assert_eq!(ends[0]["reason"], LEFT_UNENDED);
        assert_eq!(ends[1]["reason"], LEFT_UNENDED);
        assert_eq!(central.moves_reads(), 2);
    }

    // ---- be2j.a2 item 2: a reason is cut to Central's bound ----

    #[test]
    fn a_reason_over_centrals_bound_is_cut_to_it_and_the_run_ends_as_the_harness_said() {
        const HEAD: &str = "The harness failed: ";
        let ends_as = |given: String| {
            let central = Recording::default();
            let report = run_move(&central, &fails(&given), &press_tasks()).unwrap();
            assert_eq!(report["state"], "stalled", "{} bytes", given.len());
            assert_eq!(central.states(), ["stalled"], "{} bytes", given.len());
            report["reason"].as_str().unwrap().to_owned()
        };
        // 4097 bytes, one over the bound: the first words stay, and the mark that
        // it was cut counts inside the bound.
        let given = format!("{HEAD}{}", "x".repeat(4077));
        assert_eq!(given.len(), 4097);
        let kept = ends_as(given);
        assert_eq!(kept, format!("{HEAD}{} …", "x".repeat(4072)));
        assert_eq!(kept.len(), 4096);
        // 20000 bytes of a three-byte character: cut on a character boundary.
        let given = format!("{HEAD}{}", "€".repeat(6660));
        assert_eq!(given.len(), 20000);
        let kept = ends_as(given);
        assert_eq!(kept, format!("{HEAD}{} …", "€".repeat(1357)));
        assert_eq!(kept.len(), 4095);
        // Exactly the bound: kept whole.
        let given = format!("{HEAD}{}", "x".repeat(4076));
        assert_eq!(given.len(), 4096);
        assert_eq!(ends_as(given.clone()), given);
        // A cut that falls inside a line break leaves no `\r` alone, which
        // Central would refuse.
        let given = format!("{}\r\n{}", "a".repeat(4091), "b".repeat(100));
        assert_eq!(ends_as(given), format!("{} …", "a".repeat(4091)));
        // The bound is on the cleaned reason: one that is within it once cleaned is not cut.
        let given = format!("{HEAD}{}{}", "\u{200B}".repeat(100), "x".repeat(4076));
        assert_eq!(ends_as(given), format!("{HEAD}{}", "x".repeat(4076)));
    }

    #[test]
    fn centrals_words_over_the_bound_are_cut_in_the_refused_end_too() {
        let central = Recording::scripted(&[Answer::Invalid("m".repeat(4100))]);
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["state"], "refused", "{report}");
        assert_eq!(central.states(), ["done+gave", "refused"]);
        const HEAD: &str = "Central refused the record of how this run ended: ";
        let kept = report["reason"].as_str().unwrap();
        assert_eq!(kept.len(), 4096);
        assert_eq!(
            kept,
            format!("{HEAD}{} …", "m".repeat(4096 - HEAD.len() - " …".len()))
        );
    }

    // ---- be2j.a2 item 4: taking an escape out keeps the words ----

    #[test]
    fn an_escape_that_is_not_whole_loses_its_opening_and_keeps_its_words() {
        // The review's inputs (F6), each with what a person then reads.
        for (given, read) in [
            ("\u{1b}[ 12 tests failed", " 12 tests failed"),
            (
                "cost \u{9b} 12 345 euros then more",
                "cost   12 345 euros then more",
            ),
            (
                "note \u{9d} everything after this is gone",
                "note   everything after this is gone",
            ),
            (
                "\u{1b}]0;the harness died with exit 1",
                "0;the harness died with exit 1",
            ),
            ("boom \u{1b}]0;t\u{1b}[31mred\u{1b}[0m", "boom red"),
            ("text \u{1b}(B more", "text  more"),
            // A CSI goes whole in its strict form only: parameter characters
            // (U+0030–003F), then intermediate ones (U+0020–002F), then one
            // final character (U+0040–007E).
            ("a\u{1b}[?25lb\u{1b}[0 qc", "abc"),
            ("a\u{1b}[0:1;2<=>?mb", "ab"),
            ("a\u{1b}[ !\"#$%&'()*+,-./mb", "ab"),
            ("a\u{1b}[@b\u{1b}[~c", "abc"),
            ("a\u{1b}[1 2mb", "a1 2mb"),
            ("a\u{1b}[ 1mb", "a 1mb"),
            ("a\u{1b}[/\u{7f}b", "a/ b"),
            ("a\u{1b}[1?hb", "ab"),
            ("a\u{1b}[31", "a31"),
            ("a\u{1b}[", "a"),
            // The one-character openings U+009B and U+009D open nothing, whatever
            // follows them, and U+009C ends nothing: each is a control like any
            // other, and the words after it stay.
            ("a\u{9b}[31mred", "a [31mred"),
            ("a\u{9d}]0;t\u{7}b", "a ]0;t b"),
            ("x\u{1b}]0;t\u{9c}y", "x0;t y"),
            // A string sequence goes whole when it is ended on its own line: by
            // ESC `\`, by a BEL for an OSC only, or by another ESC, read as an
            // opening of its own.
            ("\u{1b}]0;window title\u{07}Done", "Done"),
            (
                "a\u{1b}]8;;https://example.test\u{1b}\\link\u{9c}b",
                "alink b",
            ),
            (
                "\u{1b}Pq#0\u{1b}\\Done, \u{1b}Pnot\u{07} ended",
                "Done, not  ended",
            ),
            (
                "a\u{1b}_hidden\u{1b}\\b\u{1b}^x\u{1b}\\c\u{1b}Xy\u{1b}\\d",
                "abcd",
            ),
            ("a\u{1b}]0;t\u{1b}(Bb", "ab"),
            ("a\u{1b}]0;t\u{1b}", "a"),
            // Without an end on its line only its two opening characters go; an
            // end on a later line is not its end.
            (
                "a\u{1b}]0;first line\nsecond\u{07} line",
                "a0;first line\nsecond  line",
            ),
            ("\u{1b}]0;a\r\nb\u{07}c", "0;a\r\nb c"),
            (
                "x\u{1b}]0;a\u{2028}b\u{07}c\u{1b}]0;d\u{2029}e\u{07}f",
                "x0;a\nb c0;d\ne f",
            ),
            ("a\u{1b}]", "a"),
            // ESC, one or more intermediate characters and one final character
            // (U+0030–007E) goes whole; any other ESC goes alone.
            ("\u{1b}(0Line", "Line"),
            ("p\u{1b}#8q\u{1b} Fr\u{1b} !~s", "pqrs"),
            // With none between it is a two-character escape, and its letter goes
            // with it (be2j.a3 item 5, as Central's cleaning has it).
            ("f\u{1b}g, h\u{1b}7i", "f, hi"),
            ("a\u{1b}(\u{7f}b", "a( b"),
            ("a\u{1b}(/bc", "ac"),
            ("a\u{1b}(/", "a(/"),
            ("a\u{1b}( \u{e9}b", "a( \u{e9}b"),
            ("a\u{1b}\u{1b}[1mb", "ab"),
            ("a\u{1b}", "a"),
        ] {
            assert_eq!(block_plain(given), read, "{given:?}");
        }
        // The run ends with the harness's words, and the sentence for a reason
        // with nothing visible in it is for those alone.
        let reason = |given: &str| {
            let central = Recording::default();
            let report = run_move(&central, &fails(given), &press_tasks()).unwrap();
            assert_eq!(central.states(), ["stalled"], "{given:?}");
            report["reason"].as_str().unwrap().to_owned()
        };
        assert_eq!(
            reason("\u{1b}]0;the harness died with exit 1"),
            "0;the harness died with exit 1"
        );
        // A reason is trimmed (be2j.a3 item 3).
        assert_eq!(reason("\u{1b}[ 12 tests failed"), "12 tests failed");
        assert_eq!(reason("boom\r"), "boom");
        const NOTHING_VISIBLE: &str = "The run's reason held nothing visible.";
        assert_eq!(reason(" \u{7} "), NOTHING_VISIBLE);
        assert_eq!(
            reason("\u{1b}[31m\u{1b}]0;title\u{7}\u{200B}"),
            NOTHING_VISIBLE
        );
        assert_eq!(reason(""), NOTHING_VISIBLE);
        assert_eq!(reason("\t\n"), NOTHING_VISIBLE);
    }

    // ---- be2j.a2 item 5: a ref is recorded as given or left out, never changed into another ----

    /// What a record says when it leaves refs out.
    fn left_out(count: usize) -> String {
        format!(
            "{} left out of this record: Central keeps no ref that is empty, over its length, or holds a character that does not show or white space where none may stand.",
            if count == 1 {
                "1 ref was".to_owned()
            } else {
                format!("{count} refs were")
            }
        )
    }

    #[test]
    fn a_ref_central_would_not_keep_is_left_out_and_the_summary_says_how_many() {
        // "return:" with a zero-width space before the 1 is not "return:1".
        let mut input = json!({"event": "end", "state": "done",
            "gave": {"summary": "Tasks: 2 proposals submitted.", "counts": {"proposed": 2},
                "proposals": ["return:\u{200B}1", "return:2", "\u{200B}", "", " ", 7, null, " return:3 ", "return:se\u{f1}al"],
                "writes": ["write:\u{202E}2", "write:\u{1b}[2Kx", "write:a\nb", "write:a\tb", "write:3"]}});
        plain_record(&mut input, None);
        assert_eq!(
            input["gave"]["proposals"],
            json!(["return:2", " return:3 ", "return:se\u{f1}al"]),
            "a ref is recorded as it was given, or not at all"
        );
        assert_eq!(input["gave"]["writes"], json!(["write:3"]));
        assert_eq!(
            input["gave"]["summary"],
            format!("Tasks: 2 proposals submitted. {}", left_out(10)).as_str()
        );
        assert_eq!(
            input["gave"]["counts"],
            json!({"proposed": 2}),
            "the counts are the run's own: no key is added"
        );
        assert!(input["gave"].get("points_at").is_none());
        // The review's four inputs (F2), and what else Central keeps no
        // `points_at` of: white space of any kind, and what is not text.
        for given in [
            json!("skill:\u{7}x"),
            json!("skill:\u{2028}x"),
            json!("skill:\tx"),
            json!("\u{200B}"),
            json!("skill:\u{2060}x"),
            json!("skill:\u{1b}[1mx"),
            json!("skill: x"),
            json!("skill:\u{a0}x"),
            json!(" skill:x"),
            json!(""),
            json!(7),
            json!(["skill:x"]),
        ] {
            let mut input = json!({"event": "end", "state": "done",
                "gave": {"summary": "Patterns: 1 proposal submitted.\n", "points_at": given}});
            plain_record(&mut input, None);
            assert!(
                input["gave"].get("points_at").is_none(),
                "{given:?} is left out, not cleaned into another ref: {input}"
            );
            assert_eq!(
                input["gave"]["summary"],
                format!("Patterns: 1 proposal submitted. {}", left_out(1)).as_str(),
                "{given:?}"
            );
        }
        // One Central keeps stays as it was given, and nothing is said.
        for given in [
            json!("skill:x"),
            json!("skill:se\u{f1}al/\u{4e00}"),
            Value::Null,
        ] {
            let mut input = json!({"event": "end", "state": "done",
                "gave": {"summary": "Patterns: 1 proposal submitted.", "points_at": given,
                    "proposals": ["return:1"], "writes": []}});
            let before = input.clone();
            plain_record(&mut input, None);
            assert_eq!(input, before, "{given:?}");
        }
        // With no summary of its own to follow, the sentence stands alone.
        for summary in [json!("\u{200B} "), Value::Null] {
            let mut input = json!({"event": "end", "state": "stalled", "reason": "r",
                "gave": {"summary": summary, "proposals": ["\u{200B}"]}});
            plain_record(&mut input, None);
            assert_eq!(input["gave"]["summary"], left_out(1).as_str());
            assert_eq!(input["gave"]["proposals"], json!([]));
        }
        // A record with no summary and nothing left out gains none.
        let mut input = json!({"event": "end", "state": "stalled", "reason": "r",
            "gave": {"proposals": ["return:1"]}});
        plain_record(&mut input, None);
        assert!(input["gave"].get("summary").is_none(), "{input}");
    }

    #[test]
    fn a_ref_central_named_with_an_invisible_character_is_left_out_of_the_runs_record() {
        let central = Recording {
            names: Some(|number| json!({"return_ref": format!("central:return:\u{200B}{number}")})),
            ..Recording::default()
        };
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "done", "{report}");
        assert_eq!(central.states(), ["done+gave"]);
        assert_eq!(report["gave"]["proposals"], json!([]), "{report}");
        assert_eq!(report["gave"]["counts"]["proposed"], 1, "{report}");
        assert_eq!(
            report["gave"]["summary"],
            format!(
                "Tasks: 1 proposal submitted for 1 entry, each waiting for you on its face. {}",
                left_out(1)
            )
            .as_str()
        );
    }

    // ---- be2j.a2 item 6: what a run counts as proposed and what its record lists agree ----

    #[test]
    fn a_proposal_central_took_and_named_no_ref_for_is_counted_and_the_summary_says_so() {
        const SUBMITTED_ONE: &str =
            "Tasks: 1 proposal submitted for 1 entry, each waiting for you on its face.";
        const SUBMITTED_TWO: &str =
            "Tasks: 2 proposals submitted for 2 entries, each waiting for you on its face.";
        const ONE_UNNAMED: &str =
            "Central named no ref for 1 proposal, so this record does not list it.";
        const TWO_UNNAMED: &str =
            "Central named no ref for 2 proposals, so this record does not list them.";
        let gave = |central: Recording, musts: &[&str]| {
            let report = run_move(&central, &proposes(musts), &press_tasks()).unwrap();
            assert_eq!(report["state"], "done", "{report}");
            assert_eq!(central.states(), ["done+gave"], "{report}");
            assert_eq!(central.submitted(), musts.len());
            report["gave"].clone()
        };
        // The review's scripted answer: a submit answered `{}`.
        let unnamed = || Recording {
            names: Some(|_| json!({})),
            ..Recording::default()
        };
        let one = gave(unnamed(), &["ring the printer"]);
        assert_eq!(one["proposals"], json!([]));
        assert_eq!(
            one["counts"],
            json!({"must": 1, "proposed": 1, "unmapped": 0})
        );
        assert_eq!(
            one["summary"],
            format!("{SUBMITTED_ONE} {ONE_UNNAMED}").as_str()
        );
        let two = gave(unnamed(), &["ring the printer", "post the letter"]);
        assert_eq!(two["proposals"], json!([]));
        assert_eq!(
            two["counts"],
            json!({"must": 2, "proposed": 2, "unmapped": 0})
        );
        assert_eq!(
            two["summary"],
            format!("{SUBMITTED_TWO} {TWO_UNNAMED}").as_str()
        );
        // One named and one not: the named one is listed, both are counted.
        let mixed = gave(
            Recording {
                names: Some(|number| match number {
                    0 => json!({"return_ref": "central:return:0"}),
                    _ => json!({"return_ref": " "}),
                }),
                ..Recording::default()
            },
            &["ring the printer", "post the letter"],
        );
        assert_eq!(mixed["proposals"], json!(["central:return:0"]));
        assert_eq!(mixed["counts"]["proposed"], 2);
        assert_eq!(
            mixed["summary"],
            format!("{SUBMITTED_TWO} {ONE_UNNAMED}").as_str()
        );
        // A proposal Central holds already is the same proposal, under the ref
        // it holds it by; held with no ref named, it is counted and not listed.
        let held = gave(
            Recording {
                held: Some(json!({"return_ref": "central:return:held"})),
                ..Recording::default()
            },
            &["ring the printer"],
        );
        assert_eq!(held["proposals"], json!(["central:return:held"]));
        assert_eq!(held["summary"], SUBMITTED_ONE);
        let held = gave(
            Recording {
                held: Some(json!({"return_ref": ""})),
                ..Recording::default()
            },
            &["ring the printer"],
        );
        assert_eq!(held["proposals"], json!([]));
        assert_eq!(held["counts"]["proposed"], 1);
        assert_eq!(
            held["summary"],
            format!("{SUBMITTED_ONE} {ONE_UNNAMED}").as_str()
        );
        // Every proposal named: the summary is the run's own, and no more.
        let named = gave(
            Recording::default(),
            &["ring the printer", "post the letter"],
        );
        assert_eq!(
            named["proposals"],
            json!(["central:return:0", "central:return:1"])
        );
        assert_eq!(named["summary"], SUBMITTED_TWO);
    }

    // ---- what a record holds is what Central keeps ----

    #[test]
    fn a_harness_failure_in_colour_ends_the_run_with_its_words_cleaned() {
        // Central refuses a reason holding any control but a line break or a tab
        // (be2j.c2): the colour codes go whole, so the run ends with the
        // harness's words, never with the guard's sentence for a run left unended.
        let central = Recording::default();
        let agent = fails("The harness said \u{1b}[31mboom\u{1b}[0m");
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        assert_eq!(report["reason"], "The harness said boom", "{report}");
        let ends = central.ends.lock().unwrap();
        assert_eq!(ends.len(), 1, "one end, the run's own: {ends:?}");
        assert_eq!(ends[0]["reason"], "The harness said boom");
    }

    /// The list Central, O-I and AIKit keep byte for byte: every control and
    /// every invisible character (its sha256 is checked below).
    fn listed() -> &'static BTreeSet<u32> {
        static LISTED: OnceLock<BTreeSet<u32>> = OnceLock::new();
        LISTED.get_or_init(|| {
            include_str!("invisible-code-points.txt")
                .trim()
                .lines()
                .map(|line| u32::from_str_radix(line, 16).unwrap())
                .collect()
        })
    }

    /// Central's `plain_characters` (`ctrl/src/continuous_work/day_moves.rs` at
    /// Central 63712618), transcribed: whether Central keeps this text, in a
    /// block (`lines`) or on one line.
    fn central_plain(text: &str, lines: bool) -> bool {
        let listed = listed();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            let kept = match c {
                '\n' | '\t' => lines,
                '\r' => lines && chars.peek() == Some(&'\n'),
                c => !listed.contains(&u32::from(c)),
            };
            if !kept {
                return false;
            }
        }
        true
    }

    fn central_list(gave: &Value, key: &str) -> std::result::Result<Vec<String>, String> {
        match gave.get(key) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(rows)) if rows.len() <= 256 => rows
                .iter()
                .map(|row| -> std::result::Result<String, String> {
                    let row = row
                        .as_str()
                        .filter(|row| !row.trim().is_empty() && row.len() <= 4096)
                        .ok_or_else(|| format!("gave.{key} is a list of refs"))?;
                    if !central_plain(row, false) {
                        return Err(format!(
                            "gave.{key} is a list of refs, each on one line, with no control character and no invisible one"
                        ));
                    }
                    Ok(row.to_owned())
                })
                .collect(),
            _ => Err(format!("gave.{key} is a list of refs")),
        }
    }

    /// Central's checks on a run's end (`move_record`, the "end" arm, at
    /// Central 63712618), transcribed from the state check on, so a record the
    /// runner makes is judged here as Central judges it. `mv` is the run's
    /// move and `agent_writes` what it began with.
    fn central_end(input: &Value, mv: &str, agent_writes: &str) -> std::result::Result<(), String> {
        let state = input["state"]
            .as_str()
            .filter(|state| !state.trim().is_empty() && state.len() <= 4096)
            .ok_or_else(|| "state requires non-empty text of at most 4096 bytes".to_owned())?;
        if !matches!(state, "done" | "refused" | "stalled" | "stopped") {
            return Err("state is done, refused, stalled or stopped".into());
        }
        let reason = match input.get("reason") {
            None | Some(Value::Null) => None,
            Some(Value::String(reason)) if !reason.trim().is_empty() && reason.len() <= 4096 => {
                if !central_plain(reason, true) {
                    return Err("reason holds no control character but line breaks and tabs (a \\r only before a \\n), and no invisible one (a line or paragraph separator, a format character or a blank filler)".into());
                }
                Some(reason.clone())
            }
            _ => return Err("reason is text, or absent".into()),
        };
        if matches!(state, "refused" | "stalled") && reason.is_none() {
            return Err(format!(
                "a run that {state} says why in a reason: nothing runs where you cannot see it"
            ));
        }
        let gave = match input.get("gave") {
            None | Some(Value::Null) => None,
            Some(gave) => {
                let object = gave.as_object().ok_or_else(|| {
                    "gave is {summary, proposals?, writes?, counts?, points_at?}".to_owned()
                })?;
                if let Some(key) = object.keys().find(|key| {
                    !matches!(
                        key.as_str(),
                        "summary" | "proposals" | "writes" | "counts" | "points_at"
                    )
                }) {
                    return Err(format!("gave has no `{key}`"));
                }
                match object.get("points_at") {
                    None | Some(Value::Null) => {}
                    Some(_) if mv != "patterns" => {
                        return Err(
                            "only the Patterns move names what its pattern's items point at".into(),
                        )
                    }
                    Some(Value::String(points_at))
                        if !points_at.is_empty()
                            && points_at.len() <= 512
                            && !points_at
                                .chars()
                                .any(|c| c.is_whitespace() || listed().contains(&u32::from(c))) => {
                    }
                    Some(_) => {
                        return Err("gave.points_at names one skill or method by its ref".into())
                    }
                }
                let summary = gave["summary"]
                    .as_str()
                    .filter(|summary| !summary.trim().is_empty() && summary.len() <= 4096)
                    .ok_or_else(|| {
                        "summary requires non-empty text of at most 4096 bytes".to_owned()
                    })?;
                if !central_plain(summary, true) {
                    return Err("gave.summary holds no control character but line breaks and tabs (a \\r only before a \\n), and no invisible one (a line or paragraph separator, a format character or a blank filler)".into());
                }
                let proposals = central_list(gave, "proposals")?;
                let writes = central_list(gave, "writes")?;
                match agent_writes {
                    "off" if !proposals.is_empty() || !writes.is_empty() => {
                        return Err("Agent writes was Off when this run began, so it gave no proposals or writes".into())
                    }
                    "propose" if !writes.is_empty() => {
                        return Err("Agent writes was Propose when this run began, so it wrote nothing directly".into())
                    }
                    _ => {}
                }
                match object.get("counts") {
                    None | Some(Value::Null) => {}
                    Some(Value::Object(counts))
                        if serde_json::to_string(counts)
                            .is_ok_and(|json| json.len() <= 16 * 1024) => {}
                    _ => return Err("gave.counts is an object".into()),
                }
                Some(())
            }
        };
        if state == "done" && gave.is_none() {
            return Err("a run that is done says what it gave".into());
        }
        Ok(())
    }

    /// The list's sha256, the same in Central (`ctrl/src/continuous_work/day_moves_tests.rs`)
    /// and O-I (`packages/live-shell/ui/tests/world-field-858.test.mjs`).
    const INVISIBLE_LIST_SHA256: &str =
        "cac89ddc2b2ce933f7bf2763663a4e94703da3cb501ef34484cc85ee4000e491";

    #[test]
    fn what_a_record_leaves_out_is_the_list_central_and_o_i_keep_and_each_field_is_cleaned() {
        use sha2::{Digest, Sha256};
        let list = include_str!("invisible-code-points.txt");
        assert_eq!(
            format!("{:x}", Sha256::digest(list.as_bytes())),
            INVISIBLE_LIST_SHA256,
            "the list is the one Central and O-I keep"
        );
        let derived: BTreeSet<u32> = (0..=0x10_FFFF)
            .filter_map(char::from_u32)
            .filter(|c| c.is_control() || invisible(*c))
            .map(u32::from)
            .collect();
        assert_eq!(
            &derived,
            listed(),
            "derived from Unicode, the set reads as the list"
        );
        // Each field of an end Central keeps plain: a block for the reason and
        // the summary; a ref as it was given, or left out.
        let mut input = json!({"event": "end", "reason": "\u{200B}\u{3164}",
            "gave": {"summary": "Tasks\u{2028}done\u{200B}, \u{0600}1\r\nnext\rthen\u{E0041}\ta\u{2029}b", "counts": {"proposed": 1},
                "proposals": ["return:\u{200B}1", "\u{200B}"], "writes": ["write:\u{202E}2"], "points_at": "skill:\u{2060}x\u{0009}"}});
        plain_record(&mut input, None);
        assert_eq!(input["reason"], "The run's reason held nothing visible.");
        assert_eq!(
            input["gave"]["summary"],
            format!("Tasks\ndone, \u{0600}1\r\nnext\nthen\ta\nb {}", left_out(4)).as_str()
        );
        assert_eq!(input["gave"]["proposals"], json!([]));
        assert_eq!(input["gave"]["writes"], json!([]));
        assert!(input["gave"].get("points_at").is_none(), "{input}");
        assert_eq!(input["gave"]["counts"], json!({"proposed": 1}));
        // A terminal's escapes go whole when they are whole (be2j.a2 item 4);
        // U+009B, U+009C and U+009D alone are controls like any other.
        assert_eq!(
            block_plain("\u{1b}[1;31mred\u{1b}[0m \u{9b}4mline\u{9b}0m"),
            "red  4mline 0m"
        );
        assert_eq!(
            block_plain("see \u{1b}]8;;https://ci.test/7\u{7}the log\u{1b}]8;;\u{1b}\\ now"),
            "see the log now"
        );
        assert_eq!(block_plain("a \u{9d}0;title\u{9c}b"), "a  0;title b");
        // One cut short loses its opening, and its words and what follows stay.
        assert_eq!(block_plain("cut \u{1b}[31\nnext"), "cut 31\nnext");
        assert_eq!(block_plain("cut \u{1b}]0;t\nnext"), "cut 0;t\nnext");
        // A block keeps its line breaks and tabs, a `\r` only before a `\n`; a
        // `\r` alone and a next line part lines (be2j.a3 item 5).
        assert_eq!(block_plain("a\tb\r\nc\rd\r"), "a\tb\r\nc\nd\n");
        assert_eq!(block_plain("a\r\r\nb\u{85}c\u{0}d"), "a\n\r\nb\nc d");
    }

    /// What is wrong, if anything, with one text as an end records it: the
    /// text in every field an agent's words reach, cleaned, and then checked
    /// as Central checks an end.
    fn recorded_wrong(text: &str) -> Option<String> {
        let show = |text: &str| text.escape_default().to_string();
        let block = block_plain(text);
        if !central_plain(&block, true) {
            return Some(format!(
                "block not plain: {} -> {}",
                show(text),
                show(&block)
            ));
        }
        if block.len() > text.len() {
            return Some(format!("cleaning grew the text: {}", show(text)));
        }
        if block_plain(&block) != block {
            return Some(format!("cleaning twice differs: {}", show(text)));
        }
        let mut input = json!({"event": "end", "state": "stalled", "reason": text,
            "gave": {"summary": format!("Patterns: {text}"), "proposals": [text],
                "writes": [text], "points_at": text}});
        plain_record(&mut input, None);
        if let Err(refusal) = central_end(&input, "patterns", "write") {
            return Some(format!(
                "Central refuses the record of {}: {refusal}",
                show(text)
            ));
        }
        let gave = &input["gave"];
        let as_given = |rows: &Value| rows == &json!([]) || rows == &json!([text]);
        if !as_given(&gave["proposals"]) || !as_given(&gave["writes"]) {
            return Some(format!("a ref of {} was changed: {gave}", show(text)));
        }
        if gave.get("points_at").is_some_and(|kept| kept != text) {
            return Some(format!("points_at of {} was changed: {gave}", show(text)));
        }
        let once = input.clone();
        plain_record(&mut input, None);
        if input != once {
            return Some(format!("recording twice differs: {}", show(text)));
        }
        None
    }

    /// The review's probe p01 (review of ai-kit #614), with each text checked
    /// as a whole record.
    #[test]
    fn every_code_point_is_recorded_as_central_keeps_it() {
        let mut wrong = Vec::new();
        for c in (0..=0x10_FFFFu32).filter_map(char::from_u32) {
            let special = listed().contains(&u32::from(c))
                || c.is_whitespace()
                || matches!(c, '[' | ']' | '\\' | '@' | '~' | '?' | '0' | 'm');
            if !special {
                // A character that shows, in no escape: it is kept, and a record of it is as given.
                let text = c.to_string();
                if block_plain(&text) != text {
                    wrong.push(format!(
                        "a character that shows was changed: {}",
                        c.escape_default()
                    ));
                }
                continue;
            }
            for context in [
                "{}",
                "a{}b",
                "{}{}",
                "\r{}",
                "{}\n",
                "{}\r",
                "a\r{}\nb",
                "\u{1b}{}x",
                "\u{1b}[{}x",
                "\u{1b}[1{}x",
                "\u{1b}]{}x",
                "\u{1b}]0;{}x\u{7}y",
                "\u{9b}{}x",
                "\u{9d}{}x",
                "{}\u{1b}",
                "{}\u{1b}[",
                "{}\u{1b}]",
                " {} ",
            ] {
                wrong.extend(recorded_wrong(&context.replace("{}", &c.to_string())));
            }
        }
        assert!(
            wrong.is_empty(),
            "{} texts are not recorded as Central keeps them, the first: {:?}",
            wrong.len(),
            wrong.iter().take(20).collect::<Vec<_>>()
        );
    }

    fn every_text(alphabet: &[char], longest: usize, each: &mut dyn FnMut(&str)) {
        let mut indexes: Vec<usize> = Vec::new();
        let mut text = String::new();
        loop {
            // The next text, counting in base `alphabet.len()` with a growing length.
            let mut place = 0;
            loop {
                if place == indexes.len() {
                    indexes.push(0);
                    break;
                }
                indexes[place] += 1;
                if indexes[place] < alphabet.len() {
                    break;
                }
                indexes[place] = 0;
                place += 1;
            }
            if indexes.len() > longest {
                return;
            }
            text.clear();
            text.extend(indexes.iter().map(|index| alphabet[*index]));
            each(&text);
        }
    }

    /// The review's probe p02: every short text over the characters an escape
    /// and a line end are made of.
    #[test]
    fn every_short_text_is_recorded_as_central_keeps_it() {
        let mut wrong = Vec::new();
        let wide = [
            '\u{1b}', '[', ']', '\\', '\u{7}', '\u{9b}', '\u{9c}', '\u{9d}', '\r', '\n', '\t',
            '\u{2028}', '\u{200b}', 'a', '1', ';', ' ', 'm',
        ];
        every_text(&wide, 4, &mut |text: &str| {
            wrong.extend(recorded_wrong(text))
        });
        let narrow = [
            '\u{1b}', '[', ']', '\\', '\u{7}', '\u{9c}', '\r', '\n', 'a', '\u{2029}',
        ];
        every_text(&narrow, 5, &mut |text: &str| {
            wrong.extend(recorded_wrong(text))
        });
        assert!(
            wrong.is_empty(),
            "{} texts are not recorded as Central keeps them, the first: {:?}",
            wrong.len(),
            wrong.iter().take(20).collect::<Vec<_>>()
        );
    }

    // ---- be2j.a3 item 6: Central's own code for a run that has ended already ----

    #[test]
    fn a_run_central_names_ended_by_its_own_code_is_the_report_and_the_day_is_not_read_again() {
        let central = Recording {
            ended_elsewhere: true,
            ended_names: Names::EndedWithTheRun,
            ..Recording::default()
        };
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["run_ref"], "run:1", "{report}");
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
        assert_eq!(
            central.states(),
            ["done+gave"],
            "one end, refused as ended; no other is sent, by the runner or by its guard"
        );
        assert_eq!(
            central.moves_reads(),
            1,
            "read to begin only: the refusal carries the run as it ended"
        );
    }

    #[test]
    fn an_ended_run_whose_refusal_carries_no_run_is_read_once_and_reported_as_it_ended() {
        let central = Recording {
            ended_elsewhere: true,
            ended_names: Names::EndedBare,
            ..Recording::default()
        };
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
        assert_eq!(central.states(), ["done+gave"]);
        assert_eq!(central.moves_reads(), 2);
    }

    // ---- be2j.a3 item 2: where the run stands ended at Central, the report says so and is no error ----

    #[test]
    fn an_ended_run_that_cannot_be_read_is_reported_ended_with_its_state_not_read() {
        // Central says the run has ended and carries no run; every later read fails.
        let central = Recording {
            ended_elsewhere: true,
            ended_names: Names::EndedBare,
            reads_fail: Some(|number| number >= 1),
            ..Recording::default()
        };
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["run_ref"], "run:1", "{report}");
        assert_eq!(report["state"], "ended, state not read", "{report}");
        assert_eq!(report["reason"], Value::Null, "{report}");
        assert_eq!(
            central.states(),
            ["done+gave"],
            "the run is ended: the guard sends nothing"
        );
        assert_eq!(central.moves_reads(), 3, "to begin, and read twice");
    }

    #[test]
    fn a_read_that_fails_after_a_conflict_is_tried_once_more() {
        // A Central before be2j.c3: the conflict's code for an ended run. The
        // first read after it fails, the second answers.
        let central = Recording {
            ended_elsewhere: true,
            reads_fail: Some(|number| number == 1),
            ..Recording::default()
        };
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(
            central.states(),
            ["done+gave"],
            "before, the failed read was taken for a run still running, and three more ends were sent"
        );
        assert_eq!(central.moves_reads(), 3);
    }

    #[test]
    fn a_run_ended_between_the_read_and_the_same_end_again_is_reported_as_it_ended() {
        // The Day changed under the first end and the run read running; it was
        // stopped before the same end arrived again.
        let central = Recording {
            script: std::sync::Mutex::new([Answer::Conflict].into_iter().collect()),
            ended_from_read: Some(2),
            ..Recording::default()
        };
        let agent = proposes(&["ring the printer"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
        assert_eq!(
            central.states(),
            ["done+gave", "done+gave"],
            "no refused end, and nothing from the guard"
        );
        assert_eq!(central.moves_reads(), 3);
        // The same, named by Central's own code: no read after it.
        let central = Recording {
            script: std::sync::Mutex::new([Answer::Conflict].into_iter().collect()),
            ended_from_read: Some(2),
            ended_names: Names::EndedWithTheRun,
            ..Recording::default()
        };
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(central.states(), ["done+gave", "done+gave"]);
        assert_eq!(central.moves_reads(), 2);
    }

    #[test]
    fn an_end_central_took_whose_answer_was_lost_is_the_report_and_not_an_error() {
        for names in [Names::Conflict, Names::EndedWithTheRun] {
            let central = Recording {
                answer_lost: true,
                ended_names: names,
                ..Recording::default()
            };
            let agent = proposes(&["ring the printer"]);
            let report = run_move(&central, &agent, &press_tasks()).unwrap();
            assert_eq!(report["state"], "done", "{report}");
            assert_eq!(report["proposals"], json!(["central:return:0"]), "{report}");
            assert_eq!(
                central.states(),
                ["done+gave"],
                "the end was taken: it is not sent again, and the guard sends nothing"
            );
            assert_eq!(central.moves_reads(), 2, "read once after the lost answer");
        }
    }

    // ---- be2j.a4 item 1: "ended, state not read" is said only of a run known ended ----

    #[test]
    fn every_end_a_conflict_and_the_day_unread_is_an_error_since_the_run_is_not_known_ended() {
        // A Central before be2j.c3 whose every end is a conflict, and no read of
        // the Day's moves answers after the first. A conflict is also the Day
        // changing, so nothing here says the run has ended.
        let central = Recording {
            rest: Some(Answer::Conflict),
            reads_fail: Some(|number| number >= 1),
            ..Recording::default()
        };
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(
            central.states(),
            ["done+gave", "done+gave", "refused", "stalled"],
            "the run's end twice, the refused end, and the guard's one try, which ends the run if it still ran"
        );
        assert_eq!(
            central.moves_reads(),
            7,
            "to begin, and twice after each conflict"
        );
    }

    #[test]
    fn a_conflict_at_the_refused_end_is_not_taken_for_an_ended_run_by_a_central_of_either_age() {
        let unread = |central: Recording| Recording {
            reads_fail: Some(|number| number >= 1),
            ..central
        };
        let refused_then =
            |answer: Answer| Recording::scripted(&[Answer::Invalid(CENTRAL_WORDS.into()), answer]);
        // The review's sequence: the run's end refused as invalid, the refused
        // end a conflict, no read answers. The run still runs, and the guard's
        // stalled is taken: the answer is an error, never "ended, state not read".
        let central = unread(refused_then(Answer::Conflict));
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(central.states(), ["done+gave", "refused", "stalled"]);
        assert_eq!(central.ends.lock().unwrap()[2]["reason"], LEFT_UNENDED);
        assert_eq!(central.moves_reads(), 3, "to begin, and twice after it");
        // The same when the read answers that the run still runs.
        let central = refused_then(Answer::Conflict);
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(central.states(), ["done+gave", "refused", "stalled"]);
        assert_eq!(central.moves_reads(), 2);
        // The run was ended elsewhere before the refused end arrived. A Central
        // with be2j.c3 names it by its own code: the run is known ended, and
        // unread it is "ended, state not read", with nothing from the guard.
        let central = unread(Recording {
            ended_after_ends: Some(2),
            ended_names: Names::EndedBare,
            ..Recording::scripted(&[Answer::Invalid(CENTRAL_WORDS.into())])
        });
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["state"], "ended, state not read", "{report}");
        assert_eq!(central.states(), ["done+gave", "refused"]);
        // An older Central names it by a conflict: unread, it is not known
        // ended, so the answer is an error and the guard has its one try.
        let central = unread(Recording {
            ended_after_ends: Some(2),
            ..Recording::scripted(&[Answer::Invalid(CENTRAL_WORDS.into())])
        });
        let error = run_move(&central, &proposes(&[]), &press_tasks()).unwrap_err();
        assert_eq!(error.code(), "day_move.central_refused");
        assert_eq!(central.states(), ["done+gave", "refused", "stalled"]);
    }

    // ---- be2j.a4 item 2: the four checks the review of #619 found unheld ----

    #[test]
    fn a_run_that_reads_ended_after_the_refused_end_met_a_conflict_is_the_report() {
        // An older Central: the run's end is refused as invalid; the run is
        // stopped from elsewhere; the refused end meets the conflict, and the
        // Day's moves show the run as it ended.
        let central = Recording {
            ended_after_ends: Some(2),
            ..Recording::scripted(&[Answer::Invalid(CENTRAL_WORDS.into())])
        };
        let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
        assert_eq!(report["state"], "stopped", "{report}");
        assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
        assert_eq!(
            central.states(),
            ["done+gave", "refused"],
            "the run is ended: nothing from the guard"
        );
        assert_eq!(central.moves_reads(), 2);
    }

    #[test]
    fn a_run_carried_in_a_refusal_is_used_only_when_it_is_this_run_and_it_has_ended() {
        // The details name an earlier run of the move, which ended done; or this
        // run as it stood while it ran. Neither is this run as it ended, so the
        // Day's moves are read, and the report is the run they hold.
        for names in [Names::EndedWithAnotherRun, Names::EndedWithTheRunRunning] {
            let central = Recording {
                ended_elsewhere: true,
                ended_names: names,
                ..Recording::default()
            };
            let report = run_move(&central, &proposes(&[]), &press_tasks()).unwrap();
            assert_eq!(report["run_ref"], "run:1", "{report}");
            assert_eq!(report["state"], "stopped", "{report}");
            assert_eq!(report["reason"], "Stopped from the Day's page.", "{report}");
            assert_eq!(central.states(), ["done+gave"]);
            assert_eq!(central.moves_reads(), 2, "to begin, and once for the run");
        }
    }

    #[test]
    fn the_first_256_of_n_counts_the_refs_a_record_could_list_and_not_the_ones_left_out() {
        let mut given: Vec<String> = (0..300).map(|n| format!("central:return:{n}")).collect();
        given.push(String::new());
        given.push("central:return:\u{200B}1".into());
        given.push("x".repeat(4097));
        let (run, ends) = recorded_done(gave_with(
            "Tasks: 303 proposals submitted.".into(),
            given.clone(),
        ));
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(run["end"]["gave"]["proposals"], json!(given[..256]));
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!(
                "Tasks: 303 proposals submitted. {} This record lists the first 256 of 300 proposals: Central keeps 256 to a record.",
                left_out(3)
            )
            .as_str()
        );
        assert_eq!(run["end"]["gave"]["counts"]["proposed"], 303);
    }

    // ---- be2j.a4 item 3: what the record says of itself is never cut off ----

    #[test]
    fn what_a_record_says_of_itself_stands_whole_after_a_summary_over_the_bound() {
        const ONE: &str = "Central named no ref for 1 proposal, so this record does not list it.";
        const TWO: &str =
            "Central named no ref for 2 proposals, so this record does not list them.";
        // The run's own words give way: they are cut, and the sentence is whole.
        let mut gave = gave_with("s".repeat(5000), vec!["central:return:0".into()]);
        gave.unnamed = 2;
        let (run, ends) = recorded_done(gave);
        assert_eq!(ends, ["done+gave"]);
        let kept = run["end"]["gave"]["summary"].as_str().unwrap();
        assert_eq!(kept.len(), 4096);
        assert_eq!(
            kept,
            format!("{} … {TWO}", "s".repeat(4096 - TWO.len() - 1 - " …".len()))
        );
        // All three sentences, in their order: what Central named no ref for,
        // what was left out, what was listed short.
        let mut given: Vec<String> = (0..300).map(|n| format!("central:return:{n}")).collect();
        given.push("central:return:\u{200B}1".into());
        let mut gave = gave_with("s".repeat(5000), given);
        gave.unnamed = 1;
        let said = format!(
            "{ONE} {} This record lists the first 256 of 300 proposals: Central keeps 256 to a record.",
            left_out(1)
        );
        let (run, ends) = recorded_done(gave);
        assert_eq!(ends, ["done+gave"]);
        let kept = run["end"]["gave"]["summary"].as_str().unwrap();
        assert_eq!(kept.len(), 4096);
        assert_eq!(
            kept,
            format!(
                "{} … {said}",
                "s".repeat(4096 - said.len() - 1 - " …".len())
            )
        );
        // A summary inside the bound is whole, the sentence after it.
        let mut gave = gave_with("Tasks: 2 proposals submitted.".into(), vec![]);
        gave.unnamed = 2;
        let (run, _) = recorded_done(gave);
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("Tasks: 2 proposals submitted. {TWO}").as_str()
        );
    }

    #[test]
    fn counts_over_centrals_bound_are_cut_to_it_and_the_summary_says_so() {
        const SAID: &str = "This record's counts were cut to the 16 KiB Central keeps of them.";
        let with_reason = |bytes: usize| {
            let mut gave = gave_with("Close: done.".into(), vec![]);
            gave.counts
                .insert("carry_reason".into(), json!("r".repeat(bytes)));
            gave
        };
        let size = |gave: &Gave| serde_json::to_string(&gave.counts).unwrap().len();
        let spare = 16 * 1024 - size(&with_reason(0));
        assert_eq!(size(&with_reason(spare)), 16 * 1024);
        // At the bound the counts are whole and nothing is said.
        let (run, ends) = recorded_done(with_reason(spare));
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(
            run["end"]["gave"]["counts"]["carry_reason"]
                .as_str()
                .unwrap()
                .len(),
            spare
        );
        assert_eq!(run["end"]["gave"]["summary"], "Close: done.");
        // One byte over: the longest words are cut, the numbers stay, the
        // summary says so, and the run ends done.
        let over = spare + 1;
        let (run, ends) = recorded_done(with_reason(over));
        assert_eq!(ends, ["done+gave"], "not refused over its counts");
        let counts = &run["end"]["gave"]["counts"];
        assert_eq!(counts["proposed"], 0);
        let kept = counts["carry_reason"].as_str().unwrap();
        assert!(kept.ends_with(" …"), "{}", kept.len());
        assert_eq!(
            kept.len(),
            over / 2,
            "half the words' length, the cut's mark inside it"
        );
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("Close: done. {SAID}").as_str()
        );
        // Nothing to cut but numbers and their names: the counts go, and the
        // record is kept.
        let mut gave = gave_with("Close: done.".into(), vec![]);
        for n in 0..2000 {
            gave.counts.insert(format!("count_{n}"), json!(n));
        }
        let (run, ends) = recorded_done(gave);
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(run["end"]["gave"]["counts"], json!({}));
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("Close: done. {SAID}").as_str()
        );
    }

    // ---- be2j.a3 item 1: a run that did its work is recorded as done, whatever its size ----

    /// A run that gave this, ended `done` through the runner's own end against
    /// the test's Central, which checks the record as Central does: what
    /// Central recorded, and the ends sent.
    fn recorded_done(gave: Gave) -> (Value, Vec<String>) {
        let central = Recording::default();
        let mut open = OpenRun {
            central: &central,
            mv: Move::Tasks,
            day_ref: "day:2026-11-10".into(),
            run_ref: "run:1".into(),
            begun_at: 1,
            armed: true,
            end_tried: false,
        };
        let ended = open.end("done", Some(&gave), None).unwrap();
        drop(open);
        (ended["run"].clone(), central.states())
    }

    fn gave_with(summary: String, proposals: Vec<String>) -> Gave {
        let mut counts = Map::new();
        counts.insert("proposed".into(), json!(proposals.len()));
        Gave {
            summary,
            proposals,
            unnamed: 0,
            counts,
        }
    }

    #[test]
    fn a_summary_over_centrals_bound_is_cut_to_it_and_the_run_ends_done_with_what_it_gave() {
        // The reviewer's measured case: a Context answer whose moments have no
        // field, each named in the summary (about 80 pass the bound; 90 here).
        let unmapped = "moment: /fields/p4_timeline or /body/p4_moments; ".repeat(90);
        let summary = format!(
            "Context: 2 proposals submitted for 92 entries, each waiting for you on its face. 90 entries had no field on the Day document to land on and were not proposed ({}).",
            unmapped.trim_end_matches("; ")
        );
        assert!(summary.len() > 4096, "{}", summary.len());
        let refs = vec!["central:return:0".to_owned(), "central:return:1".to_owned()];
        let (run, ends) = recorded_done(gave_with(summary.clone(), refs.clone()));
        assert_eq!(
            ends,
            ["done+gave"],
            "before: done refused, then the run ended refused"
        );
        assert_eq!(run["state"], "done");
        let kept = run["end"]["gave"]["summary"].as_str().unwrap();
        assert!(kept.len() <= 4096, "{}", kept.len());
        assert_eq!(kept, format!("{} …", summary[..4092].trim_end()));
        assert_eq!(run["end"]["gave"]["proposals"], json!(refs));
        // Each edge: at the bound kept whole, one byte over cut to the bound, a
        // character that straddles the cut goes whole.
        let (run, _) = recorded_done(gave_with("s".repeat(4096), vec![]));
        assert_eq!(run["end"]["gave"]["summary"], "s".repeat(4096).as_str());
        let (run, ends) = recorded_done(gave_with("s".repeat(4097), vec![]));
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("{} …", "s".repeat(4092)).as_str()
        );
        let (run, ends) = recorded_done(gave_with(format!("s{}", "€".repeat(1400)), vec![]));
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("s{} …", "€".repeat(1363)).as_str()
        );
    }

    #[test]
    fn a_ref_over_centrals_bound_is_left_out_and_counted_and_the_run_ends_done() {
        let long = format!("central:return:{}", "x".repeat(4097 - 15));
        assert_eq!(long.len(), 4097);
        let at_the_bound = long[..4096].to_owned();
        let gave = gave_with(
            "Tasks: 3 proposals submitted.".into(),
            vec!["central:return:0".into(), long, at_the_bound.clone()],
        );
        let (run, ends) = recorded_done(gave);
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(run["state"], "done");
        assert_eq!(
            run["end"]["gave"]["proposals"],
            json!(["central:return:0", at_the_bound])
        );
        assert_eq!(
            run["end"]["gave"]["summary"],
            format!("Tasks: 3 proposals submitted. {}", left_out(1)).as_str()
        );
        assert_eq!(run["end"]["gave"]["counts"]["proposed"], 3);
        // What a pattern's items point at has a bound of its own.
        for (bytes, kept) in [(512, true), (513, false)] {
            let points_at = format!("skill:{}", "x".repeat(bytes - 6));
            let mut input = json!({"event": "end", "state": "done",
                "gave": {"summary": "Patterns: 1 proposal submitted.", "points_at": points_at}});
            plain_record(&mut input, None);
            assert_eq!(input["gave"].get("points_at").is_some(), kept, "{bytes}");
            assert_eq!(
                central_end(&input, "patterns", "propose"),
                Ok(()),
                "{bytes}"
            );
        }
    }

    #[test]
    fn rows_past_centrals_bound_are_left_out_and_the_summary_says_how_many_of_how_many() {
        let refs = |count: usize| -> Vec<String> {
            (0..count).map(|n| format!("central:return:{n}")).collect()
        };
        let (run, ends) = recorded_done(gave_with(
            "Tasks: 257 proposals submitted.".into(),
            refs(257),
        ));
        assert_eq!(ends, ["done+gave"]);
        assert_eq!(run["state"], "done");
        assert_eq!(run["end"]["gave"]["proposals"], json!(refs(256)));
        assert_eq!(
            run["end"]["gave"]["summary"],
            "Tasks: 257 proposals submitted. This record lists the first 256 of 257 proposals: Central keeps 256 to a record."
        );
        assert_eq!(
            run["end"]["gave"]["counts"]["proposed"], 257,
            "the count is the run's"
        );
        // At the bound every row is listed and nothing is said.
        let (run, _) = recorded_done(gave_with(
            "Tasks: 256 proposals submitted.".into(),
            refs(256),
        ));
        assert_eq!(run["end"]["gave"]["proposals"], json!(refs(256)));
        assert_eq!(
            run["end"]["gave"]["summary"],
            "Tasks: 256 proposals submitted."
        );
        // What the record says of itself stands whole after a long summary: the
        // run's own words are cut to make room for it.
        let said =
            "This record lists the first 256 of 300 proposals: Central keeps 256 to a record.";
        let (run, ends) = recorded_done(gave_with("s".repeat(5000), refs(300)));
        assert_eq!(ends, ["done+gave"]);
        let kept = run["end"]["gave"]["summary"].as_str().unwrap();
        assert_eq!(kept.len(), 4096);
        assert_eq!(
            kept,
            format!(
                "{} … {said}",
                "s".repeat(4096 - said.len() - 1 - " …".len())
            )
        );
    }

    // ---- be2j.a3 item 3: a reason that is white space to the bound keeps its words ----

    #[test]
    fn a_reason_that_is_white_space_to_the_bound_is_trimmed_before_it_is_cut() {
        let reason = |given: String| {
            let central = Recording::default();
            let report = run_move(&central, &fails(&given), &press_tasks()).unwrap();
            assert_eq!(central.states(), ["stalled"]);
            report["reason"].as_str().unwrap().to_owned()
        };
        // The reviewer's case: before, " …" alone was recorded.
        assert_eq!(reason(format!("{}x", " ".repeat(5000))), "x");
        assert_eq!(
            reason(format!("{}the harness died\n", "\n".repeat(4200))),
            "the harness died"
        );
        // Trimmed, then cut: the first words stay.
        assert_eq!(
            reason(format!("  {}  ", "x".repeat(4097))),
            format!("{} …", "x".repeat(4092))
        );
        // White space alone shows nothing.
        assert_eq!(
            reason(" ".repeat(5000)),
            "The run's reason held nothing visible."
        );
    }

    // ---- be2j.a3 item 4: the counts where Central named no ref ----

    fn press_close() -> RunRequest {
        RunRequest {
            mv: Move::Close,
            ..press_tasks()
        }
    }

    /// The Day's agent, answering Close with a summary.
    fn closes() -> Says {
        Says(Ok(
            json!({"summary": "The day went well.", "learned": []}).to_string()
        ))
    }

    const CLOSE_SUBMITTED: &str =
        "Close: 1 proposal submitted for 1 entry, each waiting for you on its face.";
    const ONE_UNNAMED: &str =
        "Central named no ref for 1 proposal, so this record does not list it.";

    #[test]
    fn a_refused_proposals_words_count_the_proposals_central_named_no_ref_for() {
        let central = Recording {
            names: Some(|_| json!({})),
            refused_submit: Some(1),
            ..Recording::default()
        };
        let agent = proposes(&["ring the printer", "post the letter"]);
        let report = run_move(&central, &agent, &press_tasks()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        assert_eq!(
            report["reason"],
            "Central refused proposal 2 of 2 (p1_must): Central keeps no such proposal. 1 proposal already submitted wait for you.",
            "{report}"
        );
        assert_eq!(
            report["gave"]["summary"],
            format!("Tasks: 1 proposal submitted before Central refused one. {ONE_UNNAMED}")
                .as_str()
        );
        assert_eq!(report["gave"]["counts"]["proposed"], 1, "{report}");
        assert_eq!(report["gave"]["proposals"], json!([]));
    }

    #[test]
    fn a_close_stopped_part_way_keeps_the_proposals_central_named_no_ref_for() {
        // Central refuses the close.
        let central = Recording {
            names: Some(|_| json!({})),
            carry: Some("refused"),
            ..Recording::default()
        };
        let report = run_move(&central, &closes(), &press_close()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        assert_eq!(
            report["reason"],
            "Central did not close the Day: the Day is not ready to close"
        );
        assert_eq!(
            report["gave"]["summary"],
            format!("{CLOSE_SUBMITTED} The Day was not closed. {ONE_UNNAMED}").as_str()
        );
        assert_eq!(report["gave"]["counts"]["proposed"], 1, "{report}");
        // Central closes the Day and refuses the carry's proposal.
        let central = Recording {
            names: Some(|_| json!({})),
            carry: Some("waits"),
            refused_submit: Some(1),
            ..Recording::default()
        };
        let report = run_move(&central, &closes(), &press_close()).unwrap();
        assert_eq!(report["state"], "stalled", "{report}");
        assert_eq!(
            report["reason"],
            "The Day closed, but Central refused the carry proposal for tomorrow's face 0: Central keeps no such proposal"
        );
        assert_eq!(
            report["gave"]["summary"],
            format!("{CLOSE_SUBMITTED} The Day is closed; its carry to tomorrow's face 0 was not submitted. {ONE_UNNAMED}").as_str()
        );
        assert_eq!(report["gave"]["proposals"], json!([]));
    }

    #[test]
    fn closes_count_after_the_carry_counts_the_proposals_central_named_no_ref_for() {
        let central = Recording {
            names: Some(|_| json!({})),
            carry: Some("waits"),
            ..Recording::default()
        };
        let report = run_move(&central, &closes(), &press_close()).unwrap();
        assert_eq!(report["state"], "done", "{report}");
        assert_eq!(central.submitted(), 2, "the summary and the carry");
        assert_eq!(report["gave"]["proposals"], json!([]));
        assert_eq!(report["gave"]["counts"]["proposed"], 2, "{report}");
        assert_eq!(
            report["gave"]["summary"],
            format!("{CLOSE_SUBMITTED} The Day is closed; the carry to tomorrow's face 0: waits (submitted as a proposal for tomorrow's face 0). The agent named nothing the person learned. Central named no ref for 2 proposals, so this record does not list them.").as_str()
        );
    }

    // ---- be2j.a3 item 5: escapes agree with Central ----

    /// The sha256 of `escape-cases.json`, the cases Central holds too
    /// (`ctrl/src/escape-cases.json`), byte for byte: a rule that moves on one
    /// side fails that side's test of the cases, and a file that moves on one
    /// side without the other fails here or there.
    const ESCAPE_CASES_SHA256: &str =
        "c2d19384dfacf5f39610c48494b2a45ba4010a655c549d30945844edd2ea9db8";

    #[test]
    fn a_record_is_cleaned_of_escapes_as_central_cleans_a_notification() {
        use sha2::{Digest, Sha256};
        let file = include_str!("escape-cases.json");
        assert_eq!(
            format!("{:x}", Sha256::digest(file.as_bytes())),
            ESCAPE_CASES_SHA256,
            "the cases are the ones Central holds"
        );
        let cases: Value = serde_json::from_str(file).unwrap();
        let cases = cases["cases"].as_array().unwrap();
        assert!(cases.len() >= 30, "{}", cases.len());
        for case in cases {
            let name = case["name"].as_str().unwrap();
            let text = case["text"].as_str().unwrap();
            // A record's own form where it has one, else the form of a
            // notification's body.
            let record = case["record"].as_str().or(case["block"].as_str()).unwrap();
            assert_eq!(block_plain(text), record, "{name}");
            assert_eq!(recorded_wrong(text), None, "{name}");
        }
        // The three places the two sides differed before be2j.c3 and this.
        assert_eq!(block_plain("a\u{1b}7saved\u{1b}cReset"), "asavedReset");
        assert_eq!(block_plain("a\rb\u{0b}c\u{0c}d\u{85}e"), "a\nb\nc\nd\ne");
        assert_eq!(block_plain("a\u{1b}]0;t\u{0b}b\u{07}c"), "a0;t\nb c");
    }
}
