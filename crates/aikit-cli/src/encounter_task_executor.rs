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
//! 1. While the task is active, the Effort and Autonomy envelopes are played
//!    through `ModelSelect`'s `provider_reasoning_effort` and `ModeSelect`'s
//!    `provider_mode_id`, at the envelope's value for the task's elapsed time
//!    (seconds since the executor first saw this session's task bound).
//! 2. A turn's end (`HostEvent::TurnEnded`, read back as `/event/TurnEnded`
//!    on the session's own event page) is the task's own end (`after_turn`).
//!    Only a finish runs anything: a turn that ends `Completed`. A turn a
//!    person stopped (the track's Stop, the Thread's) or that failed ends the
//!    task where it stands, and the activator off launches nothing. After a
//!    finish, Loop runs first: while it is on, the executor resubmits the
//!    task's own name/purpose as a fresh turn, until the cap (the most runs,
//!    the first launch included). Loop's stop condition (Task done, Test
//!    pass) is kept with it, but no reading of a task's done or a test's pass
//!    reaches this executor, so it judges neither: a loop ends at its cap, at
//!    a turn that does not finish, or with the activator off. Once the loop
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
//!    answering a read. Solo is this module's `solo_gate`: while any session
//!    sharing a SessionSpace has `track.solo` on, every launch this executor
//!    would otherwise make (a loop's run, an Again) on a sibling that
//!    is not itself soloed is refused; a human's own `Prompt` is gated the
//!    same way, in `encounter_service.rs`.
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
//! advertised `current_value`, which would drift as the executor's own
//! selections land.
//!
//! Autonomy has no such shared vocabulary: its words
//! (`suggest`/`review`/`auto`) are AIKit's own, and a provider's permission
//! modes are its own advertised ids (ACP `availableModes`), never guaranteed
//! to carry those names. Lacking a named mapping, this executor places the
//! three words at even positions along whatever modes the resident actually
//! advertises (`proportional_index`) — the same derived placement Modulation
//! uses for its steps — and documents that standing on every selection it
//! makes, exactly as `ModeSelect`'s own receipts already do.
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
    baseline_mode: Option<String>,
    #[serde(default)]
    applied_effort: Option<String>,
    #[serde(default)]
    applied_mode: Option<String>,
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

/// Autonomy's own words, least to most autonomous. `inherit` carries no
/// position: the executor never selects a mode for it.
const AUTONOMY_WORDS: [&str; 3] = ["suggest", "review", "auto"];

fn autonomy_word_rank(word: &str) -> Option<usize> {
    AUTONOMY_WORDS.iter().position(|w| *w == word)
}

/// Place rank `rank` of `ranks` evenly across `options` (both at least 1):
/// rank 0 is the first option, the last rank is the last option, and ranks
/// between land proportionally. This is the one derived mapping both
/// Autonomy's words (onto the resident's advertised permission modes) and
/// Modulation's steps (onto whatever the resident advertises for the moved
/// envelope) are placed by; see the module doc for why neither has a named
/// one to use instead.
fn proportional_index(rank: usize, ranks: usize, options: usize) -> Option<usize> {
    if ranks == 0 || options == 0 {
        return None;
    }
    if ranks == 1 || options == 1 {
        return Some(0);
    }
    let rank = rank.min(ranks - 1);
    Some((rank * (options - 1) + (ranks - 1) / 2) / (ranks - 1))
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
/// run `loop_runs + 1` times.
fn after_turn(active: bool, task: &Value, finished: bool, loop_runs: u32) -> AfterTurn {
    if !active || !finished {
        return AfterTurn::Rest;
    }
    if task["loop"].as_bool() == Some(true)
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
    std::thread::spawn(move || loop {
        std::thread::sleep(SWEEP_INTERVAL);
        let Some(service) = weak.upgrade() else { break };
        sweep(&service, &home);
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

fn sweep(service: &EncounterService, home: &AikitHome) {
    let Ok(sessions) = known_sessions(home) else {
        return;
    };
    for session in sessions {
        // Idempotent and self-contained: a failure here (not resident, no
        // task bound, a refused native call) is simply retried next sweep.
        let _ = settle_session(service, home, &session);
    }
}

fn settle_session(
    service: &EncounterService,
    home: &AikitHome,
    session: &ResourceRef,
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
        baseline_mode: None,
        applied_effort: None,
        applied_mode: None,
        settled_through_cursor: 0,
        pending_turn_end_cursor: None,
        loop_runs: 0,
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
            baseline_mode: None,
            applied_effort: None,
            applied_mode: None,
            settled_through_cursor: 0,
            pending_turn_end_cursor: None,
            loop_runs: 0,
        };
    }

    if active {
        play_envelopes(service, session, task, &identity, &mut state);
    }
    settle_turn_end(service, home, session, task, active, &mut state)?;
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
    let autonomy_points = task["autonomy"].as_array().unwrap_or(&empty);
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
    play_autonomy(
        service,
        session,
        autonomy_points,
        modulation,
        elapsed,
        native_session_id.as_deref(),
        state,
    );
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
                let current = selector["current_value"].as_str().map(str::to_string);
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

fn play_autonomy(
    service: &EncounterService,
    session: &ResourceRef,
    points: &[Value],
    modulation: bool,
    elapsed: f64,
    native_session_id: Option<&str>,
    state: &mut ExecutorState,
) {
    let Ok(read) = service.apply(EncounterRequest::ModeRead {
        agent_session: session.clone(),
    }) else {
        return;
    };
    let observation = &read["mode_observation"];
    let options: Vec<String> = observation["available_modes"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if options.is_empty() {
        return;
    }
    let desired = if modulation {
        let baseline = match &state.baseline_mode {
            Some(baseline) => baseline.clone(),
            None => {
                let current = observation["current_mode_id"].as_str().map(str::to_string);
                state.baseline_mode = current.clone();
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
        let Some(word) = automation_level_at(points, elapsed) else {
            return;
        };
        if word == "inherit" {
            return;
        }
        let Some(rank) = autonomy_word_rank(&word) else {
            return;
        };
        let Some(index) = proportional_index(rank, AUTONOMY_WORDS.len(), options.len()) else {
            return;
        };
        options[index].clone()
    };
    if state.applied_mode.as_deref() == Some(desired.as_str()) {
        return;
    }
    if service
        .apply(EncounterRequest::ModeSelect {
            agent_session: session.clone(),
            provider_mode_id: desired.clone(),
            expected_native_session_id: native_session_id.map(str::to_string),
        })
        .is_ok()
    {
        state.applied_mode = Some(desired);
    }
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
/// last settle left off. Returns the highest `TurnEnded` seen, if any.
fn last_turn_end(
    service: &EncounterService,
    session: &ResourceRef,
    after: u64,
) -> Result<Option<TurnEnd>> {
    let mut after = after;
    let mut found: Option<(u64, bool)> = None;
    loop {
        let page = service.apply(EncounterRequest::Read {
            agent_session: session.clone(),
            after,
            limit: SCAN_LIMIT,
        })?;
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
    let after = after_turn(active, task, turn_end.finished, state.loop_runs);
    let launched = after != AfterTurn::Rest
        && solo_gate(home, session).is_ok()
        && resubmit(service, session, name).is_ok();
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
    fn autonomy_words_rank_least_to_most_autonomous() {
        assert_eq!(autonomy_word_rank("suggest"), Some(0));
        assert_eq!(autonomy_word_rank("review"), Some(1));
        assert_eq!(autonomy_word_rank("auto"), Some(2));
        assert_eq!(autonomy_word_rank("inherit"), None);
        assert_eq!(autonomy_word_rank("whatever"), None);
    }

    #[test]
    fn proportional_index_spans_the_options_evenly() {
        // Three ranks over three options: each rank keeps its own position.
        assert_eq!(proportional_index(0, 3, 3), Some(0));
        assert_eq!(proportional_index(1, 3, 3), Some(1));
        assert_eq!(proportional_index(2, 3, 3), Some(2));
        // Three ranks over two options: the middle rank lands on one side, not
        // between them (there is no "between" index), rounded to the nearer.
        let placed = (0..3)
            .map(|rank| proportional_index(rank, 3, 2).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(placed[0], 0);
        assert_eq!(placed[2], 1);
        assert!(placed[1] == 0 || placed[1] == 1);
        // Three ranks over five options: spread across the full range.
        assert_eq!(proportional_index(0, 3, 5), Some(0));
        assert_eq!(proportional_index(2, 3, 5), Some(4));
        // Degenerate inputs never panic.
        assert_eq!(proportional_index(0, 0, 5), None);
        assert_eq!(proportional_index(0, 3, 0), None);
        assert_eq!(proportional_index(0, 1, 5), Some(0));
        assert_eq!(proportional_index(9, 3, 5), proportional_index(2, 3, 5));
    }

    // REPLACED (#787): `hold_and_loop_both_keep_going_only_while_active_once_never_does` and
    // `then_actions_match_the_manuals_three_words` read Launch hold/loop as running the task again
    // and Then handoff as a send to another session. Loop is its own setting and the handoff is
    // retired (encounter_task_settings.rs `migrate` reads hold and loop as Loop on):
    // `loop_runs_the_task_again_until_its_cap_then_again_launches_it_afresh` replaces both.
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
        assert_eq!(after_turn(true, &looped, true, 0), AfterTurn::Loop);
        assert_eq!(after_turn(true, &looped, true, 500), AfterTurn::Loop);
        // The cap is the most runs, the first launch included: cap 3 runs the loop twice more.
        let capped = task(json!({"loop": true, "loop_cap": 3}));
        assert_eq!(after_turn(true, &capped, true, 0), AfterTurn::Loop);
        assert_eq!(after_turn(true, &capped, true, 1), AfterTurn::Loop);
        assert_eq!(after_turn(true, &capped, true, 2), AfterTurn::Rest);
        assert_eq!(
            after_turn(true, &task(json!({"loop": true, "loop_cap": 1})), true, 0),
            AfterTurn::Rest
        );
        // Once the loop is spent, Then follows: Again launches the task afresh; Next task and Stop
        // launch nothing here (no queue of tasks to advance to).
        let again = task(json!({"loop": true, "loop_cap": 2, "then": "again"}));
        assert_eq!(after_turn(true, &again, true, 1), AfterTurn::Again);
        assert_eq!(
            after_turn(true, &task(json!({"then": "again"})), true, 0),
            AfterTurn::Again
        );
        assert_eq!(
            after_turn(true, &task(json!({"then": "stop"})), true, 0),
            AfterTurn::Rest
        );
        assert_eq!(after_turn(true, &task(json!({})), true, 0), AfterTurn::Rest);
        // The activator off, or a turn that did not finish (a Stop, a failure), launches nothing.
        assert_eq!(after_turn(false, &looped, true, 0), AfterTurn::Rest);
        assert_eq!(
            after_turn(false, &task(json!({"then": "again"})), true, 0),
            AfterTurn::Rest
        );
        assert_eq!(after_turn(true, &looped, false, 0), AfterTurn::Rest);
        assert_eq!(
            after_turn(true, &task(json!({"then": "again"})), false, 0),
            AfterTurn::Rest
        );
        // Launch mode is how a press starts and stops the task: no mode runs it again by itself.
        for launch in ["trigger", "gate", "toggle", "repeat"] {
            assert_eq!(
                after_turn(true, &task(json!({"launch": launch})), true, 0),
                AfterTurn::Rest,
                "{launch}"
            );
        }
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
            baseline_mode: None,
            applied_effort: Some("high".into()),
            applied_mode: None,
            settled_through_cursor: 7,
            pending_turn_end_cursor: Some(8),
            loop_runs: 2,
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
            baseline_mode: None,
            applied_effort: None,
            applied_mode: None,
            settled_through_cursor: 0,
            pending_turn_end_cursor: None,
            loop_runs: 0,
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
