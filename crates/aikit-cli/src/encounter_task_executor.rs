//! The executor that makes a session's and its bound task's own settings
//! (`encounter-session-settings`, `encounter_task_settings.rs`) take effect.
//! That record is deliberately inert — "nothing here selects a model, starts
//! a turn or moves a task" — so something has to read it and act. This is
//! that something, and it lives in AIKit, beside the encounter: the acts it
//! performs (`ModelSelect`, `ModeSelect`, `Draft`, `Prompt`, `Read`) are all
//! encounter owner operations, and the running system's own kernel
//! (`desktop/cradle/kernel/src/agent_prepared.rs`) is a stateless pass-through
//! to AIKit for every one of them — it holds no task or turn state of its own
//! to drive this from. A kernel-side executor would have nothing to read.
//!
//! This module never reaches into `EncounterService`'s own fields: every act
//! it performs goes through the same public `EncounterService::apply` any
//! other caller uses, so it is "just another client" of the encounter, not a
//! second authority inside it. A background worker (`spawn_worker`) wakes on
//! a short interval, lists every session a tracked SessionSpace names
//! (`SessionSpaceApplicationStore::list`), and for each one that answers a
//! `Status` read (i.e. is actually resident) settles its task settings:
//!
//! 1. While the task is active, the Effort envelope is played through
//!    `ModelSelect`'s `provider_reasoning_effort`, and the Tool calls envelope
//!    answers the harness's requests to use a tool (`Permission`), each at the
//!    envelope's value for the task's elapsed time (seconds since the executor
//!    first saw this session's task bound).
//! 2. A turn's end (`HostEvent::TurnEnded`, read back as `/event/TurnEnded`
//!    on the session's own event page) is the task's own end (`after_turn`).
//!    Only a finish runs anything: a turn that ends `Completed`. A turn a
//!    person stopped (the track's Stop, the Thread's) or that failed ends the
//!    task where it stands, and the activator off launches nothing. After a
//!    finish, Loop runs first: while it is on, the executor resubmits the
//!    task's own name/purpose as a fresh turn, "until its stop condition or
//!    the cap" (DLW.md, The Clip panel: Loop), the cap being the most runs,
//!    the first launch included. The stop condition is read from its owners
//!    (`loop_stop`): AIKit's record of the attempts the task serves names
//!    the latest, and the Software Factory's reading of it
//!    (`factory attempt return --json`) says Task done (Factory admitted
//!    its readable Return) or Test pass (its latest verification passed).
//!    A task that serves no attempt, or a Factory that does not answer, has
//!    no verdict: its loop runs on to its cap. A loop on a song (`loop_song`,
//!    the canon unit a song's clip became on the task's track, chosen in Clip
//!    view; O-I #844, DLW manual §19.1 Songs: "It comes round again by Loop")
//!    makes each pass a take of the song: the Software Factory starts a Run
//!    of it (`factory workflow perform <state> <run> <unit> --json`,
//!    `factory.song-take/v1`), and the pass's fresh turn names that take; a
//!    take Factory refuses ends the loop there, in Factory's words, and the
//!    settings disclose either (`loop_pass`). A loop also ends at a turn
//!    that does not finish, or with the activator off. Once the loop
//!    is spent (or off), Then follows: `again` launches the task afresh, its
//!    loop counted over; `stop` does nothing further; and `next` — "the next
//!    task" — is a no-op here: this record names one bound task, never a
//!    queue of them, so there is nothing to derive an advance to; a session
//!    already rebound to a new task before this fires picks that new task up
//!    on its own, as settings always have. Launch mode (Trigger, Gate,
//!    Toggle, Repeat) is how a press starts and stops the task — the shell's
//!    press and release, which reach the encounter as `Prompt` and `cancel` —
//!    so no mode runs the task again by itself. Repeat's runs are takes, and
//!    a take is made by the run that varies them, not by resubmitting here.
//!    Then's `handoff` is retired: the work moves to another session when
//!    the person drags the task onto that session's track.
//! 3. Quantize gates exactly how eagerly that settling happens: `now` settles
//!    the moment a turn-end is observed; `global`, `next-tool` and `next-turn`
//!    all settle one sweep later. The sweep has no visibility into a single
//!    turn's own tool calls, so it cannot tell "wait for this tool call" from
//!    "wait for this turn" apart; both, and the Live-set-wide `global`
//!    reading (no transport grid drives an agent turn), collapse to the one
//!    distinction this executor can actually keep: fire on the instant, or
//!    let one boundary pass first.
//! 4. Telemetry off mutes `Read`'s own answer for that session (an edit in
//!    `encounter_service.rs`, not here, because that is where a reader's
//!    poll is served) — the session keeps working; its events stop
//!    answering a read. This executor's own scan is not a reader's poll: it
//!    reads the owner's journal (`EncounterService::owner_events`), so a
//!    muted track still settles its Loop and Again. Solo is this module's
//!    `solo_gate`: while any session sharing a SessionSpace has
//!    `track.solo` on, every launch this executor
//!    would otherwise make (a loop's run, an Again) on a sibling that
//!    is not itself soloed is refused; a human's own `Prompt` is gated the
//!    same way, in `encounter_service.rs`. A benched session (its
//!    SessionSpace's `benched`, O-I manual §19.15's Activator: "it stays in
//!    the rack and is given no work") is refused first, by `launch_gate`,
//!    the one gate both paths pass.
//!
//! Effort's envelope plays in the session's own abstract words
//! (`inherit`/`low`/`medium`/`high`/`max`, `encounter_task_settings.rs`'s
//! `EFFORTS`), sent to `ModelSelect` as `provider_reasoning_effort` directly:
//! this harness's own providers already use that exact vocabulary for their
//! reasoning-effort selector, and `ModelSelect` itself refuses a word the
//! resident never advertised, so a provider that uses another vocabulary
//! fails safely (skipped this tick, retried next) rather than silently
//! applying the wrong level. Modulation's steps (−2…+2) move an index into
//! whatever the resident actually advertises, off a baseline captured once
//! when the executor first sees the task bound and active — never off the
//! advertised `currentValue`, which would drift as the executor's own
//! selections land.
//!
//! Tool calls (O-I manual §19.1 Envelopes, §19.12 Asks: "The answer to the
//! harness's request to use a tool: ask you each time, allow for this
//! session, or allow always. You always give Deny yourself") is played as
//! that answer, in the harness's own words: while the task stands at Session
//! or Always, each request the session has pending is answered with the
//! option the harness itself offers for it (`tool_calls_answer`) — an allow
//! that names the session for Session, an allow-always that does not for
//! Always — through the encounter's own `Permission`, under the same agency
//! and context checks a person's answer passes. Ask and Inherit answer
//! nothing, so the request waits for you; a harness that offers no such
//! answer leaves it waiting too, and the settings say why (`tool_calls_play`).
//! No word ever selects a deny. The envelope answers what the harness asks:
//! it cannot make a harness ask what its own Scope already lets through.
//! The Agent's Autonomy, which this executor once placed on the resident's
//! permission modes, is retired (`encounter_task_settings.rs`).
use crate::encounter_service::{EncounterRequest, EncounterService};
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn error(message: impl std::fmt::Display) -> AikitError {
    AikitError::new("encounter.task_executor", message.to_string())
}

const STATE_SCHEMA: &str = "aikit.encounter-task-executor/v1";
/// How often the worker wakes to settle every resident session's task. There
/// is no per-turn or per-tool-call wake available to it (see the module
/// doc); this is the one boundary Quantize's `next-tool`/`next-turn`/`global`
/// settle behind.
const SWEEP_INTERVAL: Duration = Duration::from_secs(2);
/// A page of events at a time while scanning for a turn's end.
const SCAN_LIMIT: usize = 256;

/// The executor's own bookkeeping for one session: never person-authored, so
/// it carries no revision/history of its own — only what the executor has
/// already done, so a missed or repeated sweep tick stays idempotent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ExecutorState {
    schema: String,
    agent_session: ResourceRef,
    /// The task this state belongs to; a session rebound to another task
    /// restarts the task's own time and both baselines.
    task_ref: String,
    started_at_unix_ms: u128,
    #[serde(default)]
    baseline_effort: Option<String>,
    #[serde(default)]
    applied_effort: Option<String>,
    /// What Tool calls did at its last answer: the request it answered and
    /// with which of the harness's options, or why the request waits.
    #[serde(default)]
    tool_calls_play: Option<Value>,
    /// What a song's loop did at its last pass: the take Factory started
    /// (`{take, run_ref}`), or why none was (`{refused}`).
    #[serde(default)]
    song_pass: Option<Value>,
    /// Every turn-end at or before this cursor has already been settled.
    #[serde(default)]
    settled_through_cursor: u64,
    /// A turn-end seen on a previous sweep, not yet settled because Quantize
    /// is not `now`: the next sweep settles it unconditionally.
    #[serde(default)]
    pending_turn_end_cursor: Option<u64>,
    /// The runs the task's loop has launched since its last launch from
    /// elsewhere; anything but a loop's run counts it over from 0.
    #[serde(default)]
    loop_runs: u32,
}

fn state_path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state()
        .join("encounter-tasks")
        .join("executor")
        .join(format!(
            "{}.json",
            blake3::hash(session.as_str().as_bytes()).to_hex()
        ))
}

fn read_state(home: &AikitHome, session: &ResourceRef) -> Result<Option<ExecutorState>> {
    let path = state_path(home, session);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        Ok(meta) if !meta.is_file() || meta.len() > 1024 * 1024 => {
            return Err(error("Task executor state must be a bounded native file"))
        }
        Ok(_) => {}
    }
    if path.canonicalize().map_err(error)? != path {
        return Err(error("Redirected task executor state"));
    }
    let state: ExecutorState = serde_json::from_slice(&fs::read(&path).map_err(error)?)
        .map_err(|e| error(format!("Unreadable task executor state: {e}")))?;
    if state.schema != STATE_SCHEMA || &state.agent_session != session {
        return Err(error(
            "Task executor state belongs to another schema or session",
        ));
    }
    Ok(Some(state))
}

fn publish_state(home: &AikitHome, session: &ResourceRef, state: &ExecutorState) -> Result<()> {
    let target = state_path(home, session);
    let parent = target.parent().expect("executor state parent");
    fs::create_dir_all(parent).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    staged
        .write_all(&serde_json::to_vec_pretty(state).map_err(error)?)
        .map_err(error)?;
    staged.as_file().sync_all().map_err(error)?;
    staged.persist(&target).map_err(error)?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(error)
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------- pure decision logic

/// The Automation envelope's value at a second of the task: its last point at
/// or before it, else its first; an empty envelope inherits. Ported from the
/// shell's own `levelAt` (`agent/sessionSettings.ts`) so both sides agree on
/// what "the envelope's value at this second" means.
fn automation_level_at(points: &[Value], seconds: f64) -> Option<String> {
    if points.is_empty() {
        return Some("inherit".into());
    }
    let mut standing: Option<String> = None;
    for point in points {
        let pair = point.as_array()?;
        let at = pair.first()?.as_f64()?;
        let word = pair.get(1)?.as_str()?;
        if standing.is_none() || at <= seconds {
            standing = Some(word.to_string());
        }
    }
    standing
}

/// The Modulation envelope's step at a second of the task: 0 while empty,
/// else the same last-point-at-or-before rule as Automation.
fn modulation_step_at(points: &[Value], seconds: f64) -> i64 {
    if points.is_empty() {
        return 0;
    }
    let mut standing = 0i64;
    let mut first = true;
    for point in points {
        let Some(pair) = point.as_array() else {
            continue;
        };
        let Some(at) = pair.first().and_then(Value::as_f64) else {
            continue;
        };
        let Some(step) = pair.get(1).and_then(Value::as_i64) else {
            continue;
        };
        if first || at <= seconds {
            standing = step;
            first = false;
        }
    }
    standing
}

/// Scope's Tool calls for a task (O-I manual §19.1 Envelopes): which of the
/// agent's tool calls wait for you. Inherit takes the session's own, its
/// Scope's.
pub(crate) const TOOL_CALLS: &[&str] = &["inherit", "ask", "session", "always"];
/// What Autonomy's words became (§19.1 Envelopes, §19.12 Asks): the Agent's
/// Autonomy is retired, and Scope's Tool calls carries what it set.
pub(crate) const AUTONOMY_RETIRED: &str = "Autonomy is retired: Scope's Tool calls (tool_calls: inherit, ask, session, always) took its place, the answer to the harness's request to use a tool";

/// The harness's own option that answers one of its requests to use a tool
/// with Tool calls' `word`, or why the request waits for you. Session takes an
/// allow the harness names for its session (Hermes' `allow_session`, "Allow
/// for session"); Always takes an allow-always that names no session. Ask
/// and Inherit answer nothing, and nothing here ever selects a deny: "You
/// always give Deny yourself".
pub(crate) fn tool_calls_answer(
    choices: &[Value],
    word: &str,
) -> std::result::Result<String, String> {
    let named = |choice: &Value, words: &str| {
        [&choice["option_id"], &choice["label"]].iter().any(|text| {
            text.as_str()
                .is_some_and(|text| text.to_lowercase().contains(words))
        })
    };
    let allows = |choice: &&Value| {
        choice["kind"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("allow"))
    };
    let found = match word {
        "session" => choices
            .iter()
            .filter(allows)
            .find(|choice| named(choice, "session")),
        "always" => choices
            .iter()
            .filter(allows)
            .find(|choice| choice["kind"] == "allow_always" && !named(choice, "session")),
        other => {
            return Err(format!(
                "Tool calls stands at {other}: the request waits for you"
            ))
        }
    };
    let offered = || {
        choices
            .iter()
            .map(|choice| {
                choice["label"]
                    .as_str()
                    .or(choice["option_id"].as_str())
                    .unwrap_or("?")
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    match found.and_then(|choice| choice["option_id"].as_str()) {
        Some(option) => Ok(option.to_owned()),
        None if word == "session" => Err(format!(
            "The harness names no answer for this session (it offers {}), so the request waits for you",
            offered()
        )),
        None => Err(format!(
            "The harness offers no answer that allows always (it offers {}), so the request waits for you",
            offered()
        )),
    }
}

/// The value a provider-advertised select control stands at, read as the
/// native observation serializes it (`NativeConfigSelector`).
pub(crate) fn selector_current(selector: &Value) -> Option<&str> {
    selector["currentValue"].as_str()
}

/// Whether a turn-end observed this sweep may be settled now, or must wait
/// for the boundary a previous sweep already marked pending (see the module
/// doc's Quantize standing).
fn should_settle_now(quantize: &str, already_pending: bool) -> bool {
    quantize == "now" || already_pending
}

/// What a settled turn's end launches on the task's own track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterTurn {
    /// Loop is on and its cap not reached: the task runs again.
    Loop,
    /// The loop is spent or off and Then is Again: the task launches afresh.
    Again,
    /// Nothing: Next task (no queue here), Stop, the activator off, or a turn
    /// that did not finish.
    Rest,
}

/// The task's own decision at a turn's end (see the module doc): `loop_runs`
/// are the runs its loop has already launched, so with a cap the task has
/// run `loop_runs + 1` times. `stop_met` is Loop's stop condition as its
/// owner reads it (`loop_stop`): once met, the loop ends and Then follows.
fn after_turn(
    active: bool,
    task: &Value,
    finished: bool,
    loop_runs: u32,
    stop_met: bool,
) -> AfterTurn {
    if !active || !finished {
        return AfterTurn::Rest;
    }
    if task["loop"].as_bool() == Some(true)
        && !stop_met
        && task["loop_cap"]
            .as_u64()
            .is_none_or(|cap| u64::from(loop_runs) + 1 < cap)
    {
        return AfterTurn::Loop;
    }
    match task["then"].as_str() {
        Some("again") => AfterTurn::Again,
        _ => AfterTurn::Rest,
    }
}

// ------------------------------------------------------------- Loop's stop condition

/// Factory's reading of one attempt's Return, the contract Loop's stop
/// condition is read from (`factory attempt return STATE RUN ATTEMPT --json`).
const ATTEMPT_RETURN_READING: &str = "factory.attempt-return-reading/v1";

/// Where Loop's stop condition is read: the Software Factory, which owns the
/// attempts a task serves and their verdicts. AIKit's own record of the
/// attempts the task serves (`encounter-task-attempts`) names which attempt,
/// in which Run and Factory state; Factory says whether it is done or passed.
pub(crate) trait LoopVerdicts {
    /// Factory's reading of `attempt_ref`'s Return in `run_ref`.
    fn attempt_return(&self, state_path: &str, run_ref: &str, attempt_ref: &str) -> Result<Value>;
    /// A take of the song the canon unit `unit` names, in the song `run_ref` performs: the Run Factory starts for it
    /// (`factory workflow perform`). An owner that performs no song refuses in its words.
    fn perform_take(&self, _state_path: &str, _run_ref: &str, _unit: &str) -> Result<Value> {
        Err(error(
            "No Software Factory here performs a song's take (`factory workflow perform`)",
        ))
    }
}

/// Factory's take of a song, the receipt a song's loop reads.
const SONG_TAKE: &str = "factory.song-take/v1";

/// What a song's loop does at a pass: Factory started the take, and the pass's turn names it; or Factory refused, and the
/// loop ends there.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SongPass {
    Take {
        take: u64,
        run_ref: String,
        text: String,
    },
    Refused(String),
}

/// A pass of a song's loop (`task.loop_song`): Factory performs the song again, and the turn that follows names the take.
fn song_pass(name: &str, song: &Value, verdicts: &dyn LoopVerdicts) -> SongPass {
    let field = |key: &str| song[key].as_str().unwrap_or_default();
    let unit = field("unit");
    let receipt = match verdicts.perform_take(field("state_path"), field("run_ref"), unit) {
        Ok(receipt) => receipt,
        Err(why) => return SongPass::Refused(why.message().to_owned()),
    };
    let take = receipt["take"].as_u64();
    let run_ref = receipt["commission"]["commission"]["runRef"].as_str();
    match (receipt["contract"] == SONG_TAKE, take, run_ref) {
        (true, Some(take), Some(run_ref)) => SongPass::Take {
            take,
            run_ref: run_ref.to_owned(),
            text: format!("{name} — take {take} of the song {unit}: Software Factory started the Run {run_ref} for it (`factory workflow perform`), and its attempt Actions start that Run's units."),
        },
        _ => SongPass::Refused(format!(
            "Factory answered no take of the song {unit} ({SONG_TAKE}): the Factory installed here may predate `factory workflow perform`."
        )),
    }
}

/// What the executor did with the last pass of `task_ref`'s song loop on `session`, for the settings reading: the take
/// (`{take, run_ref}`), why none was made (`{refused}`), or null — no song's pass yet, or another task's.
pub(crate) fn loop_pass(home: &AikitHome, session: &ResourceRef, task_ref: &str) -> Value {
    read_state(home, session)
        .ok()
        .flatten()
        .filter(|state| state.task_ref == task_ref)
        .and_then(|state| state.song_pass)
        .unwrap_or(Value::Null)
}

/// Whether Factory's reading of an attempt meets Loop's stop condition:
/// Task done when Factory has admitted the attempt's readable Return; Test
/// pass when the attempt's latest verification passed. A reading that is not
/// Factory's attempt Return meets neither.
fn loop_condition_met(until: &str, reading: &Value) -> bool {
    if reading["contract"] != ATTEMPT_RETURN_READING {
        return false;
    }
    let record = &reading["attempt"]["record"];
    match until {
        "done" => record.get("readableReturn").is_some_and(Value::is_object),
        "pass" => record["verifications"]
            .as_array()
            .and_then(|verdicts| verdicts.last())
            .is_some_and(|latest| latest["outcome"] == "passed"),
        _ => false,
    }
}

/// Loop's stop condition for the bound task (`task.loop_until`), read from
/// its owners: the latest attempt the task serves (`attempts`, AIKit's
/// `encounter-task-attempts` reading) and Factory's verdict on it. `Ok(false)`
/// when the task serves no attempt yet; `Err` when Factory did not answer.
fn loop_stop(task: &Value, attempts: &Value, verdicts: &dyn LoopVerdicts) -> Result<bool> {
    let until = task["loop_until"].as_str().unwrap_or("done");
    // The record keeps the attempts in the order their dispatcher recorded
    // them: the last is the one the task serves now.
    let Some(latest) = attempts["attempts"].as_array().and_then(|rows| rows.last()) else {
        return Ok(false);
    };
    let field = |key: &str| {
        latest[key]
            .as_str()
            .filter(|text| !text.is_empty())
            .ok_or_else(|| error(format!("The task's attempt names no {key}")))
    };
    let reading = verdicts.attempt_return(
        field("state_path")?,
        field("run_ref")?,
        field("attempt_ref")?,
    )?;
    Ok(loop_condition_met(until, &reading))
}

/// The Software Factory installed here, asked through its own CLI with the
/// suite's override (`FACTORY_BIN`, `OI_FACTORY_BIN`) before PATH.
struct FactoryVerdicts {
    factory: String,
}

impl FactoryVerdicts {
    fn from_env() -> Self {
        Self {
            factory: crate::inhabitation::OwnerBins::from_env().factory,
        }
    }
}

impl LoopVerdicts for FactoryVerdicts {
    fn perform_take(&self, state_path: &str, run_ref: &str, unit: &str) -> Result<Value> {
        use aikit_adapters::runner::SystemRunner;
        let mut command = std::process::Command::new(&self.factory);
        command.args(["workflow", "perform", state_path, run_ref, unit, "--json"]);
        let output = SystemRunner::new()
            .with_timeout(FACTORY_WRITE_LIMIT)
            .with_output_limit_bytes(4 * 1024 * 1024)
            .capture_command(&mut command)?;
        if !output.ok() {
            return Err(error(format!(
                "Factory did not perform the take of {unit}: {}",
                refusal_words(&output.stderr)
            )));
        }
        serde_json::from_str(&output.stdout)
            .map_err(|e| error(format!("Factory's take receipt is unreadable: {e}")))
    }
    fn attempt_return(&self, state_path: &str, run_ref: &str, attempt_ref: &str) -> Result<Value> {
        use aikit_adapters::runner::SystemRunner;
        let mut command = std::process::Command::new(&self.factory);
        command.args([
            "attempt",
            "return",
            state_path,
            run_ref,
            attempt_ref,
            "--json",
        ]);
        let output = SystemRunner::new()
            .with_timeout(FACTORY_READ_LIMIT)
            .with_output_limit_bytes(4 * 1024 * 1024)
            .capture_command(&mut command)?;
        if !output.ok() {
            return Err(error(format!(
                "Factory did not read attempt {attempt_ref}: {}",
                output.stderr.trim()
            )));
        }
        serde_json::from_str(&output.stdout)
            .map_err(|e| error(format!("Factory's attempt reading is unreadable: {e}")))
    }
}

/// Factory answers a reading of its own state; the bound is for a Factory
/// that never answers, not a pace.
const FACTORY_READ_LIMIT: Duration = Duration::from_secs(15);
/// A take commissions a Run under Factory's developmental lock, which another
/// writer may hold for a while: a longer bound than a read's, still not a pace.
const FACTORY_WRITE_LIMIT: Duration = Duration::from_secs(60);

/// Factory's refusal in its own words: the message of the diagnostic it
/// writes to stderr under `--json` (`factory.workflow-diagnostic/v1`), else
/// what it wrote there.
fn refusal_words(stderr: &str) -> String {
    serde_json::from_str::<Value>(stderr.trim())
        .ok()
        .and_then(|reply| reply["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| stderr.trim().to_owned())
}

/// A turn's end is a finish when it completed; a cancelled or failed turn is
/// not (`aikit_store` encounter.rs reads the same `TurnEnded` stop).
fn turn_finished(stop: &Value) -> bool {
    stop.get("Completed").is_some() || stop.as_str() == Some("Completed")
}

/// Telemetry's own default (on) when nothing was read at all, so a reader
/// never mutes a session it could not read for an unrelated reason.
fn telemetry_default() -> bool {
    true
}

// ------------------------------------------------------------------- Read-time Telemetry

/// Whether `Read` should answer this session's events, or mute them: the
/// session's own `track.telemetry`, defaulting on when it is unread. Called
/// from `EncounterRequest::Read`'s own handler, because that is the one
/// place a reader's poll is actually served.
pub fn telemetry_on(home: &AikitHome, session: &ResourceRef) -> bool {
    match EncounterService::read_session_settings(home, session) {
        Ok(reading) => reading["track"]["telemetry"]
            .as_bool()
            .unwrap_or_else(telemetry_default),
        Err(_) => telemetry_default(),
    }
}

// --------------------------------------------------------------------------- Solo gate

/// Why a launch on `session` is refused: at least one session of the same
/// SessionSpace has Solo on, and `session` is none of them. `None` lets it
/// proceed — nothing is soloed, or `session` itself is.
fn solo_refusal(session: &ResourceRef, soloed: &[ResourceRef]) -> Option<String> {
    if soloed.is_empty() || soloed.contains(session) {
        return None;
    }
    Some(format!(
        "Solo gates launches to the soloed session(s) of this harness: {}",
        soloed
            .iter()
            .map(ResourceRef::as_str)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Every session of `session`'s own SessionSpace with Solo on, read fresh
/// (never cached: a stale Solo would either gate a launch that should run or
/// let one through that should not). A session no tracked SessionSpace names
/// solos nothing — Solo is a harness-scoped gate, not a global one.
fn soloed_siblings(home: &AikitHome, session: &ResourceRef) -> Result<Vec<ResourceRef>> {
    let spaces = SessionSpaceApplicationStore::new(home.clone()).list()?;
    let Some(space) = spaces
        .iter()
        .find(|space| space.agent_sessions.contains_key(session))
    else {
        return Ok(Vec::new());
    };
    let mut soloed = Vec::new();
    for sibling in space.agent_sessions.keys() {
        if let Ok(reading) = EncounterService::read_session_settings(home, sibling) {
            if reading["track"]["solo"].as_bool() == Some(true) {
                soloed.push(sibling.clone());
            }
        }
    }
    Ok(soloed)
}

/// Refuse a launch on `session` while another session of its SessionSpace is
/// soloed and `session` is not one of them. Called both from this executor's
/// own loop and Again runs and from `encounter_service.rs`'s `Prompt`
/// handler, so a human's own send is gated exactly the same way.
pub fn solo_gate(home: &AikitHome, session: &ResourceRef) -> Result<()> {
    let soloed = soloed_siblings(home, session)?;
    match solo_refusal(session, &soloed) {
        Some(reason) => Err(error(reason)),
        None => Ok(()),
    }
}

// --------------------------------------------------------------------------- Bench gate

/// Why a launch on `session` is refused: the SessionSpace that holds it has it
/// on the bench (O-I manual §19.15, the Agent rack's Activator: "it stays in
/// the rack and is given no work"). `None` lets it proceed. Read fresh, as
/// Solo is: a stale bench would give work to a benched session or withhold it
/// from one whose bench was lifted.
fn bench_refusal(home: &AikitHome, session: &ResourceRef) -> Result<Option<String>> {
    let spaces = SessionSpaceApplicationStore::new(home.clone()).list()?;
    Ok(spaces
        .iter()
        .find(|space| space.benched.contains(session))
        .map(|space| {
            format!(
                "{session} is on the bench of {}: it stays in the space and is given no work until its bench is lifted",
                space.id()
            )
        }))
}

/// Refuse a launch on `session` while it is benched, then while another
/// session of its SessionSpace is soloed: every launch this executor makes and
/// a human's own `Prompt` (`encounter_service.rs`) pass this one gate.
pub fn launch_gate(home: &AikitHome, session: &ResourceRef) -> Result<()> {
    if let Some(reason) = bench_refusal(home, session)? {
        return Err(error(reason));
    }
    solo_gate(home, session)
}

// --------------------------------------------------------------------------- the worker

/// Start the task executor's own worker: it wakes every `SWEEP_INTERVAL`,
/// lists every session a tracked SessionSpace names, and settles the task
/// settings of each one that answers (i.e. is actually resident). It holds
/// only a weak reference, so dropping the service ends it, exactly as the
/// conversation worker's own (`encounter_conversation.rs::spawn_worker`)
/// does — this module reads that one for precedent, but shares none of its
/// state: every act here goes through `EncounterService::apply` alone.
pub fn spawn_worker(service: &Arc<EncounterService>, home: AikitHome) {
    let weak = Arc::downgrade(service);
    let verdicts = FactoryVerdicts::from_env();
    std::thread::spawn(move || loop {
        std::thread::sleep(SWEEP_INTERVAL);
        let Some(service) = weak.upgrade() else { break };
        sweep(&service, &home, &verdicts);
    });
}

fn known_sessions(home: &AikitHome) -> Result<Vec<ResourceRef>> {
    let spaces = SessionSpaceApplicationStore::new(home.clone()).list()?;
    let mut sessions: Vec<ResourceRef> = spaces
        .iter()
        .flat_map(|space| space.agent_sessions.keys().cloned())
        .collect();
    sessions.sort();
    sessions.dedup();
    Ok(sessions)
}

fn sweep(service: &EncounterService, home: &AikitHome, verdicts: &dyn LoopVerdicts) {
    let Ok(sessions) = known_sessions(home) else {
        return;
    };
    for session in sessions {
        // Idempotent and self-contained: a failure here (not resident, no
        // task bound, a refused native call) is simply retried next sweep.
        let _ = settle_session(service, home, &session, verdicts);
    }
}

fn settle_session(
    service: &EncounterService,
    home: &AikitHome,
    session: &ResourceRef,
    verdicts: &dyn LoopVerdicts,
) -> Result<()> {
    let reading = EncounterService::read_session_settings(home, session)?;
    let Some(task) = reading.get("task").filter(|t| !t.is_null()) else {
        // No task bound: nothing of this executor's to settle. Its own
        // leftover state (a previous task's) is left for the next bind to
        // overwrite, exactly as the settings record itself discloses but
        // does not clear a previous task's settings (`previous_task`).
        return Ok(());
    };
    let task_ref = task["task_ref"].as_str().unwrap_or_default().to_string();
    let active = task["active"].as_bool().unwrap_or(true);

    // Resident check: cheap at the owner (a BTreeMap lookup before any
    // native round trip), so a bound but unopened/closed session is skipped
    // without cost beyond the one settings read already paid for above.
    let identity = match service.apply(EncounterRequest::Status {
        agent_session: session.clone(),
    }) {
        Ok(status) => status,
        Err(_) => return Ok(()),
    };

    let mut state = read_state(home, session)?.unwrap_or_else(|| ExecutorState {
        schema: STATE_SCHEMA.into(),
        agent_session: session.clone(),
        task_ref: task_ref.clone(),
        started_at_unix_ms: now_unix_ms(),
        baseline_effort: None,
        applied_effort: None,
        tool_calls_play: None,
        settled_through_cursor: 0,
        pending_turn_end_cursor: None,
        loop_runs: 0,
        song_pass: None,
    });
    if state.task_ref != task_ref {
        // A rebound session starts the task's own time over, exactly as its
        // settings already start that task's fields over.
        state = ExecutorState {
            schema: STATE_SCHEMA.into(),
            agent_session: session.clone(),
            task_ref: task_ref.clone(),
            started_at_unix_ms: now_unix_ms(),
            baseline_effort: None,
            applied_effort: None,
            tool_calls_play: None,
            settled_through_cursor: 0,
            pending_turn_end_cursor: None,
            loop_runs: 0,
            song_pass: None,
        };
    }

    if active {
        play_envelopes(service, session, task, &identity, &mut state);
    }
    settle_turn_end(service, home, session, task, active, verdicts, &mut state)?;
    publish_state(home, session, &state)
}

fn elapsed_seconds(state: &ExecutorState) -> f64 {
    ((now_unix_ms().saturating_sub(state.started_at_unix_ms)) as f64) / 1000.0
}

fn play_envelopes(
    service: &EncounterService,
    session: &ResourceRef,
    task: &Value,
    identity: &Value,
    state: &mut ExecutorState,
) {
    let modulation = task["envelope_mode"].as_str() == Some("modulation");
    let elapsed = elapsed_seconds(state);
    let empty = Vec::new();
    let effort_points = task["effort"].as_array().unwrap_or(&empty);
    let tool_calls_points = task["tool_calls"].as_array().unwrap_or(&empty);
    let native_session_id = identity["native_session_id"].as_str().map(str::to_string);

    play_effort(
        service,
        session,
        effort_points,
        modulation,
        elapsed,
        native_session_id.as_deref(),
        state,
    );
    // Modulation has no session value of Tool calls to move (the settings
    // keep Tool calls empty under it), so only Automation answers.
    if !modulation {
        play_tool_calls(
            service,
            session,
            tool_calls_points,
            elapsed,
            identity,
            state,
        );
    }
}

fn play_effort(
    service: &EncounterService,
    session: &ResourceRef,
    points: &[Value],
    modulation: bool,
    elapsed: f64,
    native_session_id: Option<&str>,
    state: &mut ExecutorState,
) {
    let Ok(read) = service.apply(EncounterRequest::ModelRead {
        agent_session: session.clone(),
    }) else {
        return;
    };
    let Some(model_id) = read["pinned_model_id"]
        .as_str()
        .or_else(|| read["model_observation"]["current_model_id"].as_str())
    else {
        return;
    };
    let selector = &read["model_observation"]["reasoning_effort"];
    let options: Vec<String> = selector["options"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["value"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if options.is_empty() {
        return;
    }
    let desired = if modulation {
        let baseline = match &state.baseline_effort {
            Some(baseline) => baseline.clone(),
            None => {
                let current = selector_current(selector).map(str::to_string);
                state.baseline_effort = current.clone();
                match current {
                    Some(current) => current,
                    None => return,
                }
            }
        };
        let step = modulation_step_at(points, elapsed);
        let base_index = options.iter().position(|o| o == &baseline).unwrap_or(0) as i64;
        let index = (base_index + step).clamp(0, options.len() as i64 - 1) as usize;
        options[index].clone()
    } else {
        match automation_level_at(points, elapsed) {
            Some(word) if word != "inherit" && options.contains(&word) => word,
            _ => return,
        }
    };
    if state.applied_effort.as_deref() == Some(desired.as_str()) {
        return;
    }
    if service
        .apply(EncounterRequest::ModelSelect {
            agent_session: session.clone(),
            provider_model_id: model_id.to_string(),
            provider_reasoning_effort: Some(desired.clone()),
            expected_native_session_id: native_session_id.map(str::to_string),
        })
        .is_ok()
    {
        state.applied_effort = Some(desired);
    }
}

/// Answer the session's pending requests to use a tool with Tool calls'
/// value at this second, in the harness's own words (`tool_calls_answer`).
/// `identity` is the session's Status reading, which names its pending
/// requests.
fn play_tool_calls(
    service: &EncounterService,
    session: &ResourceRef,
    points: &[Value],
    elapsed: f64,
    identity: &Value,
    state: &mut ExecutorState,
) {
    let Some(word) = automation_level_at(points, elapsed) else {
        return;
    };
    if word == "inherit" {
        return;
    }
    let empty = Vec::new();
    for request in identity["permissions"].as_array().unwrap_or(&empty) {
        let Some(request_id) = request["native_request_id"].as_str() else {
            continue;
        };
        let choices = request["choices"].as_array().unwrap_or(&empty);
        let play = match tool_calls_answer(choices, &word) {
            Ok(option_id) => match service.apply(EncounterRequest::Permission {
                agent_session: session.clone(),
                request_id: request_id.to_owned(),
                decision: aikit_adapters::interactive_connection::PermissionDecision::Selected {
                    option_id: option_id.clone(),
                },
            }) {
                Ok(_) => {
                    serde_json::json!({"answered": request_id, "option_id": option_id, "tool_calls": word})
                }
                Err(refused) => serde_json::json!({"waiting": request_id, "tool_calls": word,
                    "reason": format!("The encounter refused Tool calls' answer: {}", refused.message())}),
            },
            Err(reason) => {
                serde_json::json!({"waiting": request_id, "tool_calls": word, "reason": reason})
            }
        };
        state.tool_calls_play = Some(play);
    }
}

/// What a task's Tool calls did at its last answer: the request it answered
/// with the harness's own option, or why the request waits for you. Null
/// while it has answered nothing for this task.
pub(crate) fn tool_calls_play(home: &AikitHome, session: &ResourceRef, task_ref: &str) -> Value {
    read_state(home, session)
        .ok()
        .flatten()
        .filter(|state| state.task_ref == task_ref)
        .and_then(|state| state.tool_calls_play)
        .unwrap_or(Value::Null)
}

/// The latest turn's end on this session's own event page, from the cursor
/// the last settle left off.
struct TurnEnd {
    cursor: u64,
    /// The page was read through this cursor.
    scanned_through: u64,
    /// The turn completed (`turn_finished`).
    finished: bool,
}

/// A turn's end scan over this session's own event page, from the cursor the
/// last settle left off. Returns the highest `TurnEnded` seen, if any. It
/// reads the owner's journal (`owner_events`), not a reader's poll: Telemetry
/// off mutes `Read`, and a muted track still settles its Loop and Again.
fn last_turn_end(
    service: &EncounterService,
    session: &ResourceRef,
    after: u64,
) -> Result<Option<TurnEnd>> {
    let mut after = after;
    let mut found: Option<(u64, bool)> = None;
    loop {
        let page = service.owner_events(session, after, SCAN_LIMIT)?;
        let events = page["events"].as_array().cloned().unwrap_or_default();
        for row in &events {
            let cursor = row["cursor"].as_u64().unwrap_or(after);
            if let Some(ended) = row["event"].pointer("/event/TurnEnded") {
                found = Some((cursor, turn_finished(&ended["stop"])));
            }
            after = after.max(cursor);
        }
        if !page["more"].as_bool().unwrap_or(false) || events.is_empty() {
            return Ok(found.map(|(cursor, finished)| TurnEnd {
                cursor,
                scanned_through: after,
                finished,
            }));
        }
    }
}

fn settle_turn_end(
    service: &EncounterService,
    home: &AikitHome,
    session: &ResourceRef,
    task: &Value,
    active: bool,
    verdicts: &dyn LoopVerdicts,
    state: &mut ExecutorState,
) -> Result<()> {
    let Some(turn_end) = last_turn_end(service, session, state.settled_through_cursor)? else {
        return Ok(());
    };
    let quantize = task["quantize"].as_str().unwrap_or("global");
    let already_pending = state.pending_turn_end_cursor == Some(turn_end.cursor);
    if !should_settle_now(quantize, already_pending) {
        state.pending_turn_end_cursor = Some(turn_end.cursor);
        return Ok(());
    }
    state.pending_turn_end_cursor = None;
    state.settled_through_cursor = turn_end.scanned_through;

    let name = task["name"].as_str().unwrap_or("this task");
    // Loop's stop condition is read only where it decides anything: a looping
    // task's finish. Factory not answering reads as not met, so the loop runs
    // on to its cap, as every loop did before a verdict reached it.
    let stop_met = active
        && turn_end.finished
        && task["loop"].as_bool() == Some(true)
        && EncounterService::read_task_attempts(home, session)
            .and_then(|attempts| loop_stop(task, &attempts, verdicts))
            .unwrap_or(false);
    let after = after_turn(active, task, turn_end.finished, state.loop_runs, stop_met);
    let song = task.get("loop_song").filter(|song| song.is_object());
    let launched = match (after, song) {
        (AfterTurn::Rest, _) => false,
        _ if launch_gate(home, session).is_err() => false,
        // A song's loop: each pass is a take of the song, and the turn that follows names it.
        (AfterTurn::Loop, Some(song)) => match song_pass(name, song, verdicts) {
            SongPass::Take {
                take,
                run_ref,
                text,
            } => {
                state.song_pass = Some(serde_json::json!({"take": take, "run_ref": run_ref}));
                resubmit(service, session, &text).is_ok()
            }
            SongPass::Refused(why) => {
                state.song_pass = Some(serde_json::json!({"refused": why}));
                false
            }
        },
        _ => resubmit(service, session, name).is_ok(),
    };
    // A loop's count lives only across its own runs: anything else ends the
    // loop, and the task's next launch counts it over.
    state.loop_runs = match after {
        AfterTurn::Loop if launched => state.loop_runs.saturating_add(1),
        _ => 0,
    };
    Ok(())
}

/// Set a fresh draft and prompt it: the one owner path a loop's run and an
/// Again both resubmit through, reading the current draft revision
/// first because `Draft`/`Prompt` are both optimistic-concurrency writes.
fn resubmit(service: &EncounterService, session: &ResourceRef, text: &str) -> Result<()> {
    let page = service.apply(EncounterRequest::Read {
        agent_session: session.clone(),
        after: 0,
        limit: 1,
    })?;
    let basis = page["draft"]["revision"].as_u64().unwrap_or(0);
    let drafted = service.apply(EncounterRequest::Draft {
        agent_session: session.clone(),
        basis,
        text: text.to_string(),
    })?;
    let draft_revision = drafted["revision"]
        .as_u64()
        .ok_or_else(|| error("Draft did not answer a revision"))?;
    service.apply(EncounterRequest::Prompt {
        agent_session: session.clone(),
        draft_revision,
        interact: None,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(name: &str) -> ResourceRef {
        ResourceRef::parse(format!("agent-session/{name}")).unwrap()
    }

    #[test]
    fn automation_holds_the_last_point_at_or_before_the_second_else_the_first() {
        let points = json!([[0, "medium"], [30, "high"]]);
        let points = points.as_array().unwrap();
        assert_eq!(automation_level_at(points, 0.0), Some("medium".into()));
        assert_eq!(automation_level_at(points, 29.9), Some("medium".into()));
        assert_eq!(automation_level_at(points, 30.0), Some("high".into()));
        assert_eq!(automation_level_at(points, 1000.0), Some("high".into()));
        assert_eq!(automation_level_at(&[], 5.0), Some("inherit".into()));
    }

    #[test]
    fn modulation_holds_the_last_step_at_or_before_the_second_else_zero() {
        let points = json!([[0, 0], [30, 1], [60, -2]]);
        let points = points.as_array().unwrap();
        assert_eq!(modulation_step_at(points, 10.0), 0);
        assert_eq!(modulation_step_at(points, 30.0), 1);
        assert_eq!(modulation_step_at(points, 61.0), -2);
        assert_eq!(modulation_step_at(&[], 5.0), 0);
    }

    #[test]
    fn a_selectors_current_value_is_read_as_the_observation_serializes_it() {
        // Effort's Modulation baseline is this reading: read under another
        // name it is never captured, and Modulation never plays.
        let selector =
            serde_json::to_value(aikit_adapters::agent_connection::NativeConfigSelector {
                config_id: "reasoning_effort".into(),
                current_value: "medium".into(),
                options: vec![],
            })
            .unwrap();
        assert_eq!(selector_current(&selector), Some("medium"));
    }

    #[test]
    fn tool_calls_answer_in_the_harnesss_own_words_and_never_deny() {
        // Hermes names each answer (acp_adapter/permissions.py): once, session, always.
        let hermes = serde_json::json!([
            {"option_id": "allow_once", "label": "Allow once", "kind": "allow_once"},
            {"option_id": "allow_session", "label": "Allow for session", "kind": "allow_always"},
            {"option_id": "allow_always", "label": "Allow always", "kind": "allow_always"},
            {"option_id": "deny", "label": "Deny", "kind": "reject_once"},
            {"option_id": "deny_always", "label": "Deny always", "kind": "reject_always"},
        ]);
        let hermes = hermes.as_array().unwrap();
        assert_eq!(
            tool_calls_answer(hermes, "session"),
            Ok("allow_session".into())
        );
        assert_eq!(
            tool_calls_answer(hermes, "always"),
            Ok("allow_always".into())
        );
        // A harness that names no session answer: Session waits, in words that name its answers.
        let plain = serde_json::json!([
            {"option_id": "allow_always", "label": "Always Allow", "kind": "allow_always"},
            {"option_id": "allow", "label": "Allow", "kind": "allow_once"},
            {"option_id": "reject", "label": "Reject", "kind": "reject_once"},
        ]);
        let plain = plain.as_array().unwrap();
        assert_eq!(
            tool_calls_answer(plain, "always"),
            Ok("allow_always".into())
        );
        let waits = tool_calls_answer(plain, "session").unwrap_err();
        assert!(
            waits.contains("names no answer for this session")
                && waits.contains("Always Allow, Allow, Reject"),
            "{waits}"
        );
        // A gate that asks every time offers only once and deny: neither word answers it.
        let gate = serde_json::json!([
            {"option_id": "allow_once", "label": "Allow once", "kind": "allow_once"},
            {"option_id": "deny", "label": "Deny", "kind": "reject_once"},
        ]);
        let gate = gate.as_array().unwrap();
        for word in ["session", "always"] {
            let waits = tool_calls_answer(gate, word).unwrap_err();
            assert!(waits.contains("waits for you"), "{word}: {waits}");
        }
        // Ask and Inherit answer nothing; no word ever selects a deny.
        for word in ["ask", "inherit"] {
            assert!(tool_calls_answer(hermes, word).is_err(), "{word}");
        }
        let denies = serde_json::json!([
            {"option_id": "always-session-deny", "label": "Deny for session", "kind": "reject_always"},
        ]);
        assert!(tool_calls_answer(denies.as_array().unwrap(), "session").is_err());
    }

    #[test]
    fn loop_runs_the_task_again_until_its_cap_then_again_launches_it_afresh() {
        let task = |set: Value| {
            let mut task = json!({"launch": "trigger", "then": "next", "loop": false, "loop_until": "done", "loop_cap": null});
            for (key, value) in set.as_object().unwrap() {
                task[key] = value.clone();
            }
            task
        };
        // Loop on, no cap: each finish runs the task again while the activator is on.
        let looped = task(json!({"loop": true}));
        assert_eq!(after_turn(true, &looped, true, 0, false), AfterTurn::Loop);
        assert_eq!(after_turn(true, &looped, true, 500, false), AfterTurn::Loop);
        // The cap is the most runs, the first launch included: cap 3 runs the loop twice more.
        let capped = task(json!({"loop": true, "loop_cap": 3}));
        assert_eq!(after_turn(true, &capped, true, 0, false), AfterTurn::Loop);
        assert_eq!(after_turn(true, &capped, true, 1, false), AfterTurn::Loop);
        assert_eq!(after_turn(true, &capped, true, 2, false), AfterTurn::Rest);
        assert_eq!(
            after_turn(
                true,
                &task(json!({"loop": true, "loop_cap": 1})),
                true,
                0,
                false
            ),
            AfterTurn::Rest
        );
        // Once the loop is spent, Then follows: Again launches the task afresh; Next task and Stop
        // launch nothing here (no queue of tasks to advance to).
        let again = task(json!({"loop": true, "loop_cap": 2, "then": "again"}));
        assert_eq!(after_turn(true, &again, true, 1, false), AfterTurn::Again);
        assert_eq!(
            after_turn(true, &task(json!({"then": "again"})), true, 0, false),
            AfterTurn::Again
        );
        assert_eq!(
            after_turn(true, &task(json!({"then": "stop"})), true, 0, false),
            AfterTurn::Rest
        );
        assert_eq!(
            after_turn(true, &task(json!({})), true, 0, false),
            AfterTurn::Rest
        );
        // The activator off, or a turn that did not finish (a Stop, a failure), launches nothing.
        assert_eq!(after_turn(false, &looped, true, 0, false), AfterTurn::Rest);
        assert_eq!(
            after_turn(false, &task(json!({"then": "again"})), true, 0, false),
            AfterTurn::Rest
        );
        assert_eq!(after_turn(true, &looped, false, 0, false), AfterTurn::Rest);
        assert_eq!(
            after_turn(true, &task(json!({"then": "again"})), false, 0, false),
            AfterTurn::Rest
        );
        // Launch mode is how a press starts and stops the task: no mode runs it again by itself.
        for launch in ["trigger", "gate", "toggle", "repeat"] {
            assert_eq!(
                after_turn(true, &task(json!({"launch": launch})), true, 0, false),
                AfterTurn::Rest,
                "{launch}"
            );
        }
    }

    /// Factory's reading of one attempt's Return, as `factory attempt return
    /// --json` answers it (Factory attempt_task.rs read_return, attempt_view):
    /// the record in Factory's own camelCase.
    fn factory_return(record: Value) -> Value {
        json!({"contract": "factory.attempt-return-reading/v1", "runRef": "run:R1",
            "attempt": {"standing": "current-attempt", "status": "active", "record": record}})
    }

    #[test]
    fn task_done_is_factorys_admitted_return_and_test_pass_its_latest_verification() {
        let verdict = |outcome: &str| {
            json!({"verificationRef": format!("verification:{outcome}"), "ownerRef": "factory",
                "sourceRevision": "r1", "outcome": outcome, "obligations": [], "evidenceRefs": ["e:1"]})
        };
        let open = factory_return(json!({"attemptRef": "attempt:A1", "verifications": []}));
        let returned = factory_return(json!({"attemptRef": "attempt:A1", "verifications": [],
            "readableReturn": {"returnRef": "return:R1", "summary": "Done", "artifactRefs": [], "evidenceRefs": []}}));
        let passed = factory_return(json!({"attemptRef": "attempt:A1",
            "verifications": [verdict("failed"), verdict("passed")]}));
        let failed_last = factory_return(json!({"attemptRef": "attempt:A1",
            "verifications": [verdict("passed"), verdict("failed")]}));
        assert!(
            loop_condition_met("done", &returned),
            "an admitted Return is Task done"
        );
        assert!(
            !loop_condition_met("done", &open),
            "no Return yet: not done"
        );
        assert!(
            !loop_condition_met("done", &passed),
            "a pass is not a Return"
        );
        assert!(
            loop_condition_met("pass", &passed),
            "the latest verification passed"
        );
        assert!(
            !loop_condition_met("pass", &failed_last),
            "an earlier pass does not stand"
        );
        assert!(
            !loop_condition_met("pass", &open),
            "no verification: no pass"
        );
        assert!(
            !loop_condition_met("pass", &returned),
            "a Return is not a pass"
        );
        // Only Factory's attempt Return reading is judged.
        let mut other = returned.clone();
        other["contract"] = json!("factory.attempt-task-reading/v1");
        assert!(!loop_condition_met("done", &other));
        assert!(!loop_condition_met("never", &returned));
    }

    /// Factory as the test scripts it: the readings it answers, by attempt,
    /// and every call it was asked.
    struct ScriptedFactory {
        answers: std::collections::BTreeMap<String, Value>,
        asked: std::cell::RefCell<Vec<(String, String, String)>>,
    }
    impl LoopVerdicts for ScriptedFactory {
        fn attempt_return(&self, state: &str, run: &str, attempt: &str) -> Result<Value> {
            self.asked
                .borrow_mut()
                .push((state.into(), run.into(), attempt.into()));
            self.answers
                .get(attempt)
                .cloned()
                .ok_or_else(|| error("Factory: attempt not retained"))
        }
    }

    #[test]
    fn a_loop_reads_its_stop_from_the_latest_attempt_the_task_serves() {
        let attempt = |n: u32| {
            json!({"task_ref": "task:fixtures", "attempt_ref": format!("attempt:A{n}"),
                "run_ref": "run:R1", "state_path": "/factory/state.json", "workflow_unit": null,
                "recorded_by": "factory", "recorded_at_unix_ms": n})
        };
        let attempts = json!({"schema": "aikit.encounter-task-attempts/v1", "task_ref": "task:fixtures",
            "attempts": [attempt(1), attempt(2)], "earlier": []});
        let factory = ScriptedFactory {
            answers: [
                (
                    "attempt:A1".to_string(),
                    factory_return(json!({"verifications": [],
                    "readableReturn": {"returnRef": "return:old", "summary": "old"}})),
                ),
                (
                    "attempt:A2".to_string(),
                    factory_return(json!({"verifications": []})),
                ),
            ]
            .into(),
            asked: Default::default(),
        };
        let done = json!({"loop": true, "loop_until": "done"});
        // The latest attempt (A2) has no Return yet: the earlier one's does not end the loop.
        assert!(!loop_stop(&done, &attempts, &factory).unwrap());
        assert_eq!(
            factory.asked.borrow().last().unwrap(),
            &(
                "/factory/state.json".to_string(),
                "run:R1".to_string(),
                "attempt:A2".to_string()
            )
        );
        let returned = ScriptedFactory {
            answers: [(
                "attempt:A2".to_string(),
                factory_return(json!({"verifications": [],
                    "readableReturn": {"returnRef": "return:new", "summary": "new"}})),
            )]
            .into(),
            asked: Default::default(),
        };
        assert!(loop_stop(&done, &attempts, &returned).unwrap());
        assert!(!loop_stop(
            &json!({"loop": true, "loop_until": "pass"}),
            &attempts,
            &returned
        )
        .unwrap());
        // A task that serves no attempt has no verdict yet: the loop runs on to its cap.
        let none = json!({"attempts": [], "earlier": [attempt(1)]});
        assert!(!loop_stop(&done, &none, &returned).unwrap());
        // Factory not answering is said, never read as a verdict.
        let silent = ScriptedFactory {
            answers: Default::default(),
            asked: Default::default(),
        };
        assert!(loop_stop(&done, &attempts, &silent).is_err());
        // The stop condition decides the loop: met, the loop ends and Then follows.
        let looped = json!({"loop": true, "loop_until": "done", "loop_cap": null, "then": "next"});
        assert_eq!(after_turn(true, &looped, true, 0, true), AfterTurn::Rest);
        let again = json!({"loop": true, "loop_until": "pass", "loop_cap": 9, "then": "again"});
        assert_eq!(after_turn(true, &again, true, 3, true), AfterTurn::Again);
        assert_eq!(after_turn(true, &again, true, 3, false), AfterTurn::Loop);
    }

    /// Factory as a song's loop meets it: each take it starts, numbered in order, or its refusal in its words.
    struct ScriptedTakes {
        refuse_after: Option<u64>,
        asked: std::cell::RefCell<Vec<(String, String, String)>>,
    }
    impl LoopVerdicts for ScriptedTakes {
        fn attempt_return(&self, _state: &str, _run: &str, _attempt: &str) -> Result<Value> {
            Err(error("Factory: no attempt here"))
        }
        fn perform_take(&self, state: &str, run: &str, unit: &str) -> Result<Value> {
            let mut asked = self.asked.borrow_mut();
            asked.push((state.into(), run.into(), unit.into()));
            let take = asked.len() as u64;
            if self.refuse_after.is_some_and(|after| take > after) {
                return Err(error("Factory did not perform the take: The set release-check names has moved since it was rendered. Nothing is performed."));
            }
            Ok(
                json!({"contract": SONG_TAKE, "take": take, "unit": unit, "of": run,
                "commission": {"commission": {"runRef": format!("run:TAKE{take}")}}, "execution": "not-requested"}),
            )
        }
    }

    #[test]
    fn a_loop_on_a_song_takes_the_song_again_each_pass_through_factorys_perform() {
        // O-I #844: "It comes round again by Loop" — each pass a take of its own, a Run Factory starts.
        let song = json!({"state_path": "/walk/taker/.factory/development-state.json", "run_ref": "run:R1", "unit": "release-check"});
        let factory = ScriptedTakes {
            refuse_after: Some(2),
            asked: Default::default(),
        };
        let first = song_pass("verify the change", &song, &factory);
        assert_eq!(
            first,
            SongPass::Take {
                take: 1,
                run_ref: "run:TAKE1".into(),
                text: "verify the change — take 1 of the song release-check: Software Factory started the Run run:TAKE1 for it (`factory workflow perform`), and its attempt Actions start that Run's units.".into(),
            }
        );
        assert_eq!(
            factory.asked.borrow().first().unwrap(),
            &(
                "/walk/taker/.factory/development-state.json".to_string(),
                "run:R1".to_string(),
                "release-check".to_string()
            ),
            "the take is the song the loop names, in the Run it stands in"
        );
        assert!(matches!(
            song_pass("verify the change", &song, &factory),
            SongPass::Take { take: 2, .. }
        ));
        // Factory refuses the third: the loop ends there, in Factory's words, and nothing is resubmitted.
        match song_pass("verify the change", &song, &factory) {
            SongPass::Refused(why) => {
                assert!(why.contains("has moved since it was rendered"), "{why}")
            }
            other => panic!("a refused take ends the loop: {other:?}"),
        }
        // An answer that is no take of a song is said, never read as one.
        struct Elsewhere;
        impl LoopVerdicts for Elsewhere {
            fn attempt_return(&self, _: &str, _: &str, _: &str) -> Result<Value> {
                Err(error("none"))
            }
            fn perform_take(&self, _: &str, _: &str, _: &str) -> Result<Value> {
                Ok(json!({"contract": "factory.workflow-edit/v1"}))
            }
        }
        assert!(
            matches!(song_pass("t", &song, &Elsewhere), SongPass::Refused(why) if why.contains(SONG_TAKE))
        );
        // Factory's refusal reaches the loop in its words: the diagnostic it writes under --json, else its stderr.
        assert_eq!(
            refusal_words(
                r#"{"contract":"factory.workflow-diagnostic/v1","error":{"code":"workflow.song_take","message":"release-check is a task, not a song. Nothing is performed."}}"#
            ),
            "release-check is a task, not a song. Nothing is performed."
        );
        assert_eq!(
            refusal_words("  Unknown workflow operation perform\n"),
            "Unknown workflow operation perform"
        );
        // A Factory that predates perform refuses through the trait's own words.
        let old = ScriptedFactory {
            answers: Default::default(),
            asked: Default::default(),
        };
        assert!(matches!(song_pass("t", &song, &old), SongPass::Refused(_)));
    }

    #[test]
    fn the_settings_disclose_what_the_executor_did_with_the_last_pass_of_a_songs_loop() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap());
        let session = session("song-loop");
        assert_eq!(
            loop_pass(&home, &session, "task:fixtures"),
            Value::Null,
            "no pass yet"
        );
        let mut state = ExecutorState {
            schema: STATE_SCHEMA.into(),
            agent_session: session.clone(),
            task_ref: "task:fixtures".into(),
            started_at_unix_ms: 1,
            baseline_effort: None,
            applied_effort: None,
            tool_calls_play: None,
            settled_through_cursor: 0,
            pending_turn_end_cursor: None,
            loop_runs: 1,
            song_pass: Some(json!({"take": 3, "run_ref": "run:TAKE3"})),
        };
        publish_state(&home, &session, &state).unwrap();
        assert_eq!(
            loop_pass(&home, &session, "task:fixtures"),
            json!({"take": 3, "run_ref": "run:TAKE3"})
        );
        assert_eq!(
            loop_pass(&home, &session, "task:other"),
            Value::Null,
            "another task's pass is not this one's"
        );
        state.song_pass = Some(json!({"refused": "Factory did not perform the take."}));
        publish_state(&home, &session, &state).unwrap();
        assert_eq!(
            loop_pass(&home, &session, "task:fixtures")["refused"],
            "Factory did not perform the take."
        );
        // And what Tool calls did at its last answer, or why the request waits.
        assert_eq!(
            tool_calls_play(&home, &session, "task:fixtures"),
            Value::Null
        );
        state.tool_calls_play = Some(json!({"waiting": "permission-1", "tool_calls": "session",
            "reason": "The harness names no answer for this session (it offers Allow, Reject), so the request waits for you"}));
        publish_state(&home, &session, &state).unwrap();
        assert_eq!(
            tool_calls_play(&home, &session, "task:fixtures")["waiting"],
            "permission-1"
        );
        assert_eq!(tool_calls_play(&home, &session, "task:other"), Value::Null);
    }

    #[test]
    fn only_a_completed_turn_is_a_finish() {
        assert!(turn_finished(
            &json!({"Completed": {"stop_reason": "end_turn"}})
        ));
        assert!(turn_finished(&json!({"Completed": "EndTurn"})));
        assert!(turn_finished(&json!({"Completed": {}})));
        assert!(!turn_finished(&json!("Cancelled")));
        assert!(!turn_finished(&json!({"Cancelled": {}})));
        assert!(!turn_finished(
            &json!({"Failed": {"message": "provider exited"}})
        ));
        assert!(!turn_finished(&Value::Null));
    }

    #[test]
    fn quantize_now_settles_at_once_the_others_wait_one_sweep() {
        assert!(should_settle_now("now", false));
        assert!(should_settle_now("now", true));
        assert!(!should_settle_now("next-turn", false));
        assert!(!should_settle_now("next-tool", false));
        assert!(!should_settle_now("global", false));
        assert!(should_settle_now("next-turn", true));
        assert!(should_settle_now("next-tool", true));
        assert!(should_settle_now("global", true));
    }

    #[test]
    fn solo_refuses_every_session_but_the_soloed_ones() {
        let a = session("a");
        let b = session("b");
        assert_eq!(solo_refusal(&a, &[]), None, "nothing soloed: nothing gated");
        assert_eq!(
            solo_refusal(&a, std::slice::from_ref(&a)),
            None,
            "the soloed session itself runs"
        );
        assert!(
            solo_refusal(&b, std::slice::from_ref(&a)).is_some(),
            "every other session is gated"
        );
        assert!(
            solo_refusal(&b, &[a, b.clone()]).is_none(),
            "soloed among several: it runs too"
        );
    }

    #[test]
    fn telemetry_defaults_on_and_an_unread_session_is_never_muted() {
        assert!(telemetry_default());
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        // No SessionSpace tracks this session at all: read_session_settings
        // refuses it, and that refusal must never read as "muted".
        assert!(telemetry_on(&home, &session("unread")));
    }

    #[test]
    fn executor_state_is_published_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        let who = session("state-roundtrip");
        assert!(read_state(&home, &who).unwrap().is_none());
        let state = ExecutorState {
            schema: STATE_SCHEMA.into(),
            agent_session: who.clone(),
            task_ref: "task:fixtures".into(),
            started_at_unix_ms: 1_700_000_000_000,
            baseline_effort: Some("medium".into()),
            applied_effort: Some("high".into()),
            tool_calls_play: None,
            settled_through_cursor: 7,
            pending_turn_end_cursor: Some(8),
            loop_runs: 2,
            song_pass: None,
        };
        publish_state(&home, &who, &state).unwrap();
        let back = read_state(&home, &who).unwrap().unwrap();
        assert_eq!(back, state);
        // A state written before the loop was counted reads with its count at 0.
        let mut earlier = serde_json::to_value(&state).unwrap();
        earlier.as_object_mut().unwrap().remove("loop_runs");
        fs::write(
            state_path(&home, &who),
            serde_json::to_vec(&earlier).unwrap(),
        )
        .unwrap();
        assert_eq!(read_state(&home, &who).unwrap().unwrap().loop_runs, 0);
    }

    /// A home with `name` attached to a retained SessionSpace, as the owner's
    /// own reads require.
    fn attached(name: &str) -> (tempfile::TempDir, AikitHome, ResourceRef) {
        use aikit_core::session_space::SessionSpaceRef;
        use aikit_core::session_space_application::{
            SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
        };
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        home.ensure_layout().unwrap();
        let owner = SessionSpaceApplicationStore::new(home.clone());
        let space = SessionSpaceRef::parse(&format!("session-space/{name}")).unwrap();
        let who = session(name);
        let create = owner
            .stage(
                None,
                SessionSpaceMutation::Create {
                    id: space.clone(),
                    label: None,
                },
            )
            .unwrap();
        owner.apply(&create).unwrap();
        let attach = owner
            .stage(
                Some(&space),
                SessionSpaceMutation::AttachAgentSession {
                    attachment: SessionSpaceAgentAttachmentIntent {
                        agent_session: who.clone(),
                        purpose: Some("task executor regression".into()),
                        provenance: vec!["isolated executor test".into()],
                    },
                },
            )
            .unwrap();
        owner.apply(&attach).unwrap();
        (dir, home, who)
    }

    #[test]
    fn a_benched_session_is_given_no_work_and_its_bench_lifted_it_runs_again() {
        use aikit_core::session_space::SessionSpaceRef;
        use aikit_core::session_space_application::SessionSpaceMutation;
        let (_dir, home, who) = attached("benched");
        launch_gate(&home, &who).expect("an unbenched session runs");
        let owner = SessionSpaceApplicationStore::new(home.clone());
        let space = SessionSpaceRef::parse("session-space/benched").unwrap();
        for benched in [true, false] {
            let staged = owner
                .stage(
                    Some(&space),
                    SessionSpaceMutation::BenchAgentSession {
                        agent_session: who.clone(),
                        benched,
                    },
                )
                .unwrap();
            owner.apply(&staged).unwrap();
            let gated = launch_gate(&home, &who);
            if benched {
                let refused = gated.expect_err("a benched session is given no work");
                assert!(
                    refused
                        .to_string()
                        .contains("agent-session/benched is on the bench of session-space/benched"),
                    "{refused}"
                );
            } else {
                gated.expect("its bench lifted, it runs again");
            }
        }
        assert_eq!(
            owner.history(&space).unwrap().len(),
            4,
            "create, attach, bench, lift: each kept"
        );
    }

    #[test]
    fn a_muted_track_still_settles_its_own_turn_ends() {
        let (_dir, home, who) = attached("muted");
        EncounterService::write_session_settings(
            &home,
            &who,
            json!({"track": {"telemetry": false}}),
            None,
        )
        .unwrap();
        aikit_store::encounter::EncounterStore::open(&home)
            .unwrap()
            .append(
                &who,
                &json!({"event": {"TurnEnded": {"agent_session": who.as_str(),
                    "stop": {"Completed": {"stop_reason": "end_turn"}}}}}),
            )
            .unwrap();
        let service = EncounterService::new(home.clone()).unwrap();
        // A reader's poll is muted: Telemetry off withholds the events.
        let page = service
            .apply(EncounterRequest::Read {
                agent_session: who.clone(),
                after: 0,
                limit: 16,
            })
            .unwrap();
        assert_eq!(page["events"], json!([]), "a reader hears nothing");
        // The executor's own scan is the owner settling its task, not a
        // reader's poll: a muted track still settles its Loop and Again.
        let end = last_turn_end(&service, &who, 0)
            .unwrap()
            .expect("the muted session's turn end is seen");
        assert!(end.finished);
        assert!(end.cursor >= 1);
    }

    #[test]
    fn executor_state_from_another_session_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        let who = session("owner");
        let mismatched = ExecutorState {
            schema: STATE_SCHEMA.into(),
            agent_session: session("someone-else"),
            task_ref: "task:fixtures".into(),
            started_at_unix_ms: 0,
            baseline_effort: None,
            applied_effort: None,
            tool_calls_play: None,
            settled_through_cursor: 0,
            pending_turn_end_cursor: None,
            loop_runs: 0,
            song_pass: None,
        };
        // Published at who's own path despite naming another session.
        let path = state_path(&home, &who);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&mismatched).unwrap()).unwrap();
        assert!(read_state(&home, &who)
            .unwrap_err()
            .to_string()
            .contains("another schema or session"));
    }
}
