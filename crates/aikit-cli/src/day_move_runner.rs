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
//!    run that was begun is always ended.
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
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;
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
        ResidentOwner::at(crate::encounter_service::socket_path(&self.home))
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
        if self.owner(&EncounterRequest::Health).is_err() {
            crate::encounter_service::start(&self.home, &self.cwd).map_err(|error| {
                TurnFailure::Failed(format!(
                    "the encounter owner is not running and could not be started: {}",
                    error.message()
                ))
            })?;
        }
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
    /// (a retry, or a lost answer) is the same proposal, returned again.
    fn submit(
        &self,
        producer_key: &str,
        target: &Value,
        brief: &str,
    ) -> std::result::Result<String, CentralError> {
        let input = self.input(producer_key, target, brief);
        let submitted = self.central.call("central.receiving.submit", &input);
        match submitted {
            Ok(received) => Ok(received["return_ref"]
                .as_str()
                .unwrap_or_default()
                .to_owned()),
            Err(error) if !error.unavailable => {
                match self.central.call(
                    "central.receiving.read",
                    &json!({"producer_key": producer_key}),
                ) {
                    Ok(held) if held["return_ref"].is_string() => {
                        Ok(held["return_ref"].as_str().unwrap_or_default().to_owned())
                    }
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
    ) -> std::result::Result<String, CentralError> {
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
    counts: Map<String, Value>,
}

impl Gave {
    fn json(&self) -> Value {
        json!({"summary": self.summary, "proposals": self.proposals, "counts": self.counts})
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
            Ok(return_ref) => {
                gave.proposals.push(return_ref);
                let held = by_kind.get(item.kind).and_then(Value::as_u64).unwrap_or(0);
                by_kind.insert(item.kind.to_owned(), json!(held + 1));
            }
            Err(error) => {
                let reason = format!(
                    "Central refused proposal {} of {} ({}): {error}. {} already submitted wait for you.",
                    position + 1,
                    resolved.len(),
                    item.field_id,
                    count_words(gave.proposals.len(), "proposal", "proposals"),
                );
                gave.summary = format!(
                    "{}: {} submitted before Central refused one.",
                    mv.label(),
                    count_words(gave.proposals.len(), "proposal", "proposals")
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
            count_words(gave.proposals.len(), "proposal", "proposals"),
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
    counts.insert("proposed".into(), json!(gave.proposals.len()));
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
            Ok(return_ref) => {
                gave.proposals.push(return_ref);
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
        .insert("proposed".into(), json!(gave.proposals.len()));
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

/// A begun run is ended even when the runner stops without ending it (a panic,
/// an early return): it ends `stalled` and says so. `finish` disarms it.
struct OpenRun<'a> {
    central: &'a dyn CentralPort,
    run_ref: String,
    begun_at: i64,
    armed: bool,
}

impl OpenRun<'_> {
    fn end(&mut self, state: &str, gave: Option<&Gave>, reason: Option<&str>) -> Result<Value> {
        let mut input = json!({"event": "end", "run_ref": self.run_ref,
            "at_unix_ms": now_ms().max(self.begun_at), "state": state});
        if let Some(gave) = gave {
            input["gave"] = gave.json();
        }
        if let Some(reason) = reason {
            input["reason"] = json!(reason);
        }
        let ended = self
            .central
            .call("central.day.move.record", &input)
            .map_err(central_failure);
        if ended.is_ok() {
            self.armed = false;
        }
        ended
    }
}

impl Drop for OpenRun<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.end(
                "stalled",
                None,
                Some("The runner stopped before it recorded how this run ended."),
            );
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

/// Run one move once. An `Err` means the run could not be recorded at all;
/// every run that was begun comes back as a report with its recorded end.
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
        run_ref: run_ref.clone(),
        begun_at: run["begun_at_unix_ms"].as_i64().unwrap_or(begun_at),
        armed: true,
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
}
