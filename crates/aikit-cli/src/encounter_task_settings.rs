//! An agent session's own settings, beside its bound task: how its track
//! plays (Telemetry, Solo) and how its task launches, what follows it and how
//! the agent works in it (name, colour, activator, output style, Loop with its
//! stop condition and cap, Launch mode, Then, Quantize, what a Repeat varies,
//! the Effort and Autonomy envelopes, and its source: the constellation it was
//! made of). The task record stays the
//! admission's; these settings are the person's, kept under their own revision
//! with every replaced reading in history. Task settings bind to the task they
//! were written for (`task_ref`): a rebound session starts from the defaults
//! and discloses the previous task's settings. Settings are intent. Nothing
//! here selects a model, starts a turn or moves a task; the owners of those
//! acts read these settings.
//!
//! The words are the manual's (DLW.md, Clip view: "The Clip panel" and "The
//! Launch panel"): Launch mode Trigger · Gate · Toggle · Repeat, Then Next
//! task · Again · Stop, Loop Off or On with its stop condition (Task done,
//! Test pass) and its cap, and for Repeat its Variable, Levels, Repeats and
//! Stop when. "Once the first run starts, these do not change. A change
//! starts a new set of takes": a Repeat's takes start and end here
//! (`takes`, by whoever runs them), keeping the four fields as they stood at
//! the first run, and a change to them is refused while the takes run.
//! Hand off to and Then's handoff are retired: "To hand the work to
//! another session, drag the task onto that session's track" — the drag sends
//! the task to that session through its encounter's own Draft and Prompt.
//! Settings stored in the earlier words read in these (`migrate`).
use super::{error, read as read_task_record};
use crate::encounter_service::EncounterService;
use aikit_core::{ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions, SessionSpaceApplicationStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{fs, io::Write, path::PathBuf};

pub const READING_SCHEMA: &str = "aikit.encounter-session-settings/v1";
pub const LIST_SCHEMA: &str = "aikit.encounter-session-settings-list/v1";
const MAX_LISTED: usize = 256;
const RECORD_SCHEMA: &str = "aikit.encounter-session-settings-record/v1";
const MAX_POINTS: usize = 64;
const MAX_SECONDS: f64 = 7.0 * 24.0 * 3600.0;

const REGISTERS: &[&str] = &["plain", "warm", "terse"];
const FORMS: &[&str] = &["technical", "narrative", "list"];
const LAUNCHES: &[&str] = &["trigger", "gate", "toggle", "repeat"];
const THENS: &[&str] = &["next", "again", "stop"];
/// Loop's stop condition: Task done, Test pass.
const LOOP_UNTILS: &[&str] = &["done", "pass"];
/// The most runs a loop makes, its first launch included.
const MAX_LOOP_CAP: u64 = 1000;
/// Repeat's Variable, its Stop when, and the bounds the manual sets on its
/// Levels (1–3 values) and Repeats (1–32).
const VARIABLES: &[&str] = &[
    "condition",
    "model",
    "effort",
    "skillset",
    "budget",
    "autonomy",
];
const REPEAT_STOPS: &[&str] = &["turns", "time", "done", "pass"];
const MAX_LEVELS: usize = 3;
const MAX_LEVEL_CHARS: usize = 64;
const MAX_REPEATS: u64 = 32;
const HANDOFF_RETIRED: &str = "Then's handoff and Hand off to are retired: to hand the work to another session, drag the task onto that session's track";
const QUANTIZES: &[&str] = &["global", "now", "next-tool", "next-turn"];
const MODES: &[&str] = &["automation", "modulation"];
const EFFORTS: &[&str] = &["inherit", "low", "medium", "high", "max"];
const AUTONOMIES: &[&str] = &["inherit", "suggest", "review", "auto"];
/// Modulation raises or lowers the session's own value by these steps.
const STEPS: std::ops::RangeInclusive<i64> = -2..=2;

/// The stored record: only what the person set. Defaults are stated on read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsRecord {
    schema: String,
    agent_session: ResourceRef,
    revision: SourceRevision,
    #[serde(default)]
    track: Map<String, Value>,
    #[serde(default)]
    task: Option<TaskSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskSettings {
    task_ref: ResourceRef,
    #[serde(default)]
    set: Map<String, Value>,
    /// The set of takes a Repeat runs, with the fields it froze when its
    /// first run started. Absent until a Repeat's takes start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    takes: Option<TakesSet>,
}

/// One set of takes a Repeat runs (DLW.md, The Launch panel: "Once the first
/// run starts, these do not change. A change starts a new set of takes."):
/// Repeat's Variable, Levels, Repeats and Stop when as they stood when its
/// first run started, and how the set ended. Whoever runs the takes starts
/// and ends the set (the shell's Repeat press, through Actuation's
/// comparison run); AIKit keeps it and refuses the frozen fields' changes
/// while it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TakesSet {
    trace_ref: String,
    started_at_unix_ms: u64,
    variable: Value,
    levels: Value,
    repeats: Value,
    stop: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ended: Option<TakesEnd>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TakesEnd {
    at_unix_ms: u64,
    /// made · failed · stopped.
    outcome: String,
    /// The takes the set made, as the run counted them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    takes: Option<u64>,
    /// Where the run keeps its takes (Actuation's comparison root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// The person's change. `null` returns a field to its default.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsRequest {
    #[serde(default)]
    track: Option<Map<String, Value>>,
    #[serde(default)]
    task: Option<TaskRequest>,
    #[serde(default)]
    takes: Option<TakesRequest>,
}

/// A Repeat's takes starting or ending, by whoever runs them.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
enum TakesRequest {
    Start {
        task_ref: ResourceRef,
        trace_ref: String,
    },
    End {
        task_ref: ResourceRef,
        trace_ref: String,
        outcome: String,
        #[serde(default)]
        takes: Option<u64>,
        #[serde(default)]
        root: Option<String>,
        #[serde(default)]
        reason: Option<String>,
    },
}

/// How a set of takes ends.
const TAKES_OUTCOMES: &[&str] = &["made", "failed", "stopped"];
/// The fields a set of takes freezes, as the settings name them.
const REPEAT_FIELDS: &[&str] = &[
    "repeat_variable",
    "repeat_levels",
    "repeat_repeats",
    "repeat_stop",
];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest {
    task_ref: ResourceRef,
    set: Map<String, Value>,
}

/// The bound task as the settings see it: its identity, its purpose (the
/// name a task has before the person names it) and the constellation it was
/// made of (its source before the person sets another).
#[derive(Debug, Clone, PartialEq)]
struct BoundTask {
    task_ref: ResourceRef,
    purpose: String,
    source: Option<String>,
}

fn path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state()
        .join("encounter-tasks")
        .join("settings")
        .join(format!(
            "{}.json",
            blake3::hash(session.as_str().as_bytes()).to_hex()
        ))
}

fn read_record(home: &AikitHome, session: &ResourceRef) -> Result<Option<SettingsRecord>> {
    let path = path(home, session);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        Ok(meta) if !meta.is_file() || meta.len() > 1024 * 1024 => {
            return Err(error("Session settings must be a bounded native file"))
        }
        Ok(_) => {}
    }
    if path.canonicalize().map_err(error)? != path {
        return Err(error("Redirected session settings"));
    }
    let record: SettingsRecord = serde_json::from_slice(&fs::read(&path).map_err(error)?)
        .map_err(|e| error(format!("Unreadable session settings: {e}")))?;
    if record.schema != RECORD_SCHEMA || &record.agent_session != session {
        return Err(error(
            "Session settings belong to another schema or session",
        ));
    }
    let mut record = record;
    if let Some(task) = record.task.as_mut() {
        migrate(&mut task.set);
    }
    Ok(Some(record))
}

/// Settings stored in the earlier words read in the manual's. Launch `once`
/// is Trigger. `hold` and `loop` both ran the task again after each finish
/// while it was active: `hold` is Gate and `loop` is Trigger, each with Loop
/// on. Then's `handoff` took the work off this track, so nothing followed on
/// it: Stop. Hand off to is retired with it. The record on disk keeps its
/// words until the next write publishes these, the replaced reading going to
/// history as every write's does.
fn migrate(set: &mut Map<String, Value>) {
    let (launch, looped) = match set.get("launch").and_then(Value::as_str) {
        Some("once") => ("trigger", false),
        Some("hold") => ("gate", true),
        Some("loop") => ("trigger", true),
        _ => ("", false),
    };
    if !launch.is_empty() {
        set.insert("launch".into(), json!(launch));
    }
    if looped {
        set.entry("loop").or_insert(json!(true));
    }
    if set.get("then").and_then(Value::as_str) == Some("handoff") {
        set.insert("then".into(), json!("stop"));
    }
    set.remove("handoff_to");
}

/// Publish by atomic replacement, keeping the replaced reading in history.
fn publish(home: &AikitHome, session: &ResourceRef, record: &SettingsRecord) -> Result<()> {
    let target = path(home, session);
    let parent = target.parent().expect("settings parent");
    fs::create_dir_all(parent).map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(error)?;
    }
    if target.exists() {
        let previous = fs::read(&target).map_err(error)?;
        let history = parent.join("history");
        fs::create_dir_all(&history).map_err(error)?;
        let entry = history.join(format!("{}.json", blake3::hash(&previous).to_hex()));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&entry)
        {
            Ok(mut file) => {
                file.write_all(&previous).map_err(error)?;
                file.sync_all().map_err(error)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if fs::read(&entry).map_err(error)? != previous {
                    return Err(error("Session settings history conflict"));
                }
            }
            Err(e) => return Err(error(e)),
        }
    }
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(error)?;
    staged
        .write_all(&serde_json::to_vec_pretty(record).map_err(error)?)
        .map_err(error)?;
    staged.as_file().sync_all().map_err(error)?;
    staged.persist(&target).map_err(error)?;
    fs::File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(error)
}

fn one_of(field: &str, value: &Value, words: &[&str]) -> Result<Value> {
    match value.as_str() {
        Some(word) if words.contains(&word) => Ok(json!(word)),
        _ => Err(error(format!("{field} takes one of {}", words.join(", ")))),
    }
}

/// Launch mode and Then, with what became of an earlier word said where one
/// is sent.
fn launch_word(value: &Value) -> Result<Value> {
    one_of("Launch", value, LAUNCHES).map_err(|refused| match value.as_str() {
        Some("once" | "hold" | "loop") => error(format!(
            "{refused}: once is now trigger and hold is gate; loop is the task's own Loop (loop: true)"
        )),
        _ => refused,
    })
}

fn then_word(value: &Value) -> Result<Value> {
    one_of("Then", value, THENS).map_err(|refused| match value.as_str() {
        Some("handoff") => error(format!("{refused}. {HANDOFF_RETIRED}")),
        _ => refused,
    })
}

fn count(field: &str, value: &Value, max: u64) -> Result<Value> {
    value
        .as_u64()
        .filter(|n| (1..=max).contains(n))
        .map(|n| json!(n))
        .ok_or_else(|| error(format!("{field} is a whole number from 1 to {max}")))
}

/// Repeat's Levels: the values the variable takes, each one group of takes.
fn levels(value: &Value) -> Result<Value> {
    let refused = || {
        error(format!(
            "Repeat's levels are 1 to {MAX_LEVELS} distinct values, each text of 1 to {MAX_LEVEL_CHARS} characters"
        ))
    };
    let rows = value
        .as_array()
        .filter(|rows| (1..=MAX_LEVELS).contains(&rows.len()))
        .ok_or_else(refused)?;
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        let level = words("A level", row, MAX_LEVEL_CHARS).map_err(|_| refused())?;
        if kept.contains(&level) {
            return Err(refused());
        }
        kept.push(level);
    }
    Ok(Value::Array(kept))
}

/// Repeat's Levels are values of its Variable: where AIKit keeps the
/// variable's words (Effort, Autonomy), each level is one of them. A
/// condition, a model, a skillset and a budget are their own owners' values,
/// which the run that applies them judges; AIKit keeps no copy of them.
fn check_repeat(set: &Map<String, Value>) -> Result<()> {
    let variable = set
        .get("repeat_variable")
        .and_then(Value::as_str)
        .unwrap_or("condition");
    let words = match variable {
        "effort" => EFFORTS,
        "autonomy" => AUTONOMIES,
        _ => return Ok(()),
    };
    let Some(levels) = set.get("repeat_levels").and_then(Value::as_array) else {
        return Ok(());
    };
    if levels
        .iter()
        .all(|level| level.as_str().is_some_and(|level| words.contains(&level)))
    {
        return Ok(());
    }
    Err(error(format!(
        "With {variable} varied, each of Repeat's levels is one of {}; send the levels with the variable",
        words.join(", ")
    )))
}

/// The longest wiki ref a task keeps as its source.
const MAX_SOURCE_CHARS: usize = 1024;

/// The constellation a task was made of (O-I manual §19.1 The Clip panel:
/// "Constellation — The one the task was made of — The task's source"; O-I
/// #709, #768): the wiki ref of its whole, as the Wiki names it (`wiki:…`).
/// It is the task's source, as a sample is an audio clip's: nothing here
/// reads the wiki, and the task's work leaves the constellation as it is.
pub(super) fn constellation(value: &Value) -> Result<Value> {
    match value.as_str() {
        Some(r)
            if r.len() > "wiki:".len()
                && r.starts_with("wiki:")
                && r.chars().count() <= MAX_SOURCE_CHARS
                && r == r.trim()
                && !r.chars().any(char::is_control) =>
        {
            Ok(json!(r))
        }
        _ => Err(error(format!(
            "A task's source is the wiki ref of the constellation it was made of (wiki:…), at most {MAX_SOURCE_CHARS} characters"
        ))),
    }
}

fn flag(field: &str, value: &Value) -> Result<Value> {
    value
        .as_bool()
        .map(Value::Bool)
        .ok_or_else(|| error(format!("{field} is on or off (true or false)")))
}

fn words(field: &str, value: &Value, max: usize) -> Result<Value> {
    match value.as_str().map(str::trim) {
        Some(text)
            if !text.is_empty()
                && text.chars().count() <= max
                && !text.chars().any(char::is_control) =>
        {
            Ok(json!(text))
        }
        _ => Err(error(format!(
            "{field} is text of 1 to {max} characters, without control characters"
        ))),
    }
}

/// An envelope's points: `[[second, value]…]` along the task's time, strictly
/// increasing in time. Automation holds the control's words; Modulation holds
/// steps from −2 to +2 off the session's own value.
fn points(field: &str, value: &Value, modulation: bool, levels: &[&str]) -> Result<Value> {
    let rows = value
        .as_array()
        .filter(|rows| rows.len() <= MAX_POINTS)
        .ok_or_else(|| error(format!("{field} is a list of at most {MAX_POINTS} points")))?;
    let mut last = f64::NEG_INFINITY;
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        let pair = row
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(|| error(format!("Each point of {field} is [second, value]")))?;
        let at = pair[0]
            .as_f64()
            .filter(|at| at.is_finite() && (0.0..=MAX_SECONDS).contains(at) && *at > last)
            .ok_or_else(|| {
                error(format!(
                    "The points of {field} lie at increasing seconds from the task's start"
                ))
            })?;
        last = at;
        let step = if modulation {
            pair[1]
                .as_i64()
                .filter(|step| STEPS.contains(step))
                .map(|step| json!(step))
                .ok_or_else(|| {
                    error(format!(
                        "Modulation moves {field} by steps from -2 to 2; send the points with the mode"
                    ))
                })?
        } else {
            one_of(&format!("Each point of {field}"), &pair[1], levels).map_err(|_| {
                error(format!(
                    "Automation sets {field} to one of {}; send the points with the mode",
                    levels.join(", ")
                ))
            })?
        };
        kept.push(json!([at, step]));
    }
    Ok(Value::Array(kept))
}

fn merge(into: &mut Map<String, Value>, changes: &Map<String, Value>) {
    for (key, value) in changes {
        if value.is_null() {
            into.remove(key);
        } else {
            into.insert(key.clone(), value.clone());
        }
    }
}

fn validate_track(changes: &Map<String, Value>) -> Result<Map<String, Value>> {
    let mut kept = Map::new();
    for (key, value) in changes {
        let value = match (key.as_str(), value) {
            (_, Value::Null) => Value::Null,
            ("telemetry", value) => flag("Telemetry", value)?,
            ("solo", value) => flag("Solo", value)?,
            (other, _) => {
                return Err(error(format!(
                    "A track has no setting {other}; it takes telemetry and solo"
                )))
            }
        };
        kept.insert(key.clone(), value);
    }
    Ok(kept)
}

/// Validate each changed task field on its own; the envelope points are
/// checked against the merged mode, and Repeat's levels against the merged
/// variable, afterwards.
fn validate_task(changes: &Map<String, Value>) -> Result<Map<String, Value>> {
    let mut kept = Map::new();
    for (key, value) in changes {
        let value = match (key.as_str(), value) {
            ("handoff_to", _) => return Err(error(HANDOFF_RETIRED)),
            (_, Value::Null) => Value::Null,
            ("name", value) => words("Name", value, 200)?,
            ("colour", value) => words("Colour", value, 64)?,
            ("active", value) => flag("The clip activator", value)?,
            ("register", value) => one_of("Register", value, REGISTERS)?,
            ("form", value) => one_of("Form", value, FORMS)?,
            ("loop", value) => flag("Loop", value)?,
            ("loop_until", value) => one_of("Loop's stop condition", value, LOOP_UNTILS)?,
            ("loop_cap", value) => count("Loop's cap", value, MAX_LOOP_CAP)?,
            ("launch", value) => launch_word(value)?,
            ("then", value) => then_word(value)?,
            ("quantize", value) => one_of("Quantize", value, QUANTIZES)?,
            ("repeat_variable", value) => one_of("Repeat's variable", value, VARIABLES)?,
            ("repeat_levels", value) => levels(value)?,
            ("repeat_repeats", value) => count("Repeat's repeats", value, MAX_REPEATS)?,
            ("repeat_stop", value) => one_of("Repeat's stop condition", value, REPEAT_STOPS)?,
            ("envelope_mode", value) => one_of("The envelope mode", value, MODES)?,
            ("envelope_linked", value) => flag("The envelope link", value)?,
            ("source", value) => constellation(value)?,
            // Checked against the merged mode once every change is in.
            ("effort", value) | ("autonomy", value) => value.clone(),
            (other, _) => {
                return Err(error(format!(
                    "A task has no setting {other}; it takes name, colour, active, register, form, loop, loop_until, loop_cap, launch, then, quantize, repeat_variable, repeat_levels, repeat_repeats, repeat_stop, envelope_mode, envelope_linked, source, effort and autonomy"
                )))
            }
        };
        kept.insert(key.clone(), value);
    }
    Ok(kept)
}

fn check_envelopes(set: &mut Map<String, Value>) -> Result<()> {
    let modulation = set.get("envelope_mode").and_then(Value::as_str) == Some("modulation");
    for (field, levels) in [("effort", EFFORTS), ("autonomy", AUTONOMIES)] {
        if let Some(value) = set.get(field).cloned() {
            set.insert(field.into(), points(field, &value, modulation, levels)?);
        }
    }
    Ok(())
}

fn bound_task(home: &AikitHome, session: &ResourceRef) -> Result<Option<BoundTask>> {
    Ok(read_task_record(home, session)?.map(|record| BoundTask {
        task_ref: record.request.central.task_ref.clone(),
        purpose: record.request.central.purpose.clone(),
        source: record.request.source.clone(),
    }))
}

/// Repeat's fields as they stand: what the person set, else the defaults the
/// reading states.
fn repeat_value(set: &Map<String, Value>, key: &str) -> Value {
    set.get(key).cloned().unwrap_or_else(|| match key {
        "repeat_variable" => json!("condition"),
        "repeat_levels" => json!([]),
        "repeat_repeats" => json!(3),
        _ => json!("done"),
    })
}

fn takes_reading(takes: Option<&TakesSet>) -> Value {
    takes.map_or(Value::Null, |takes| {
        json!({
            "trace_ref": takes.trace_ref,
            "started_at_unix_ms": takes.started_at_unix_ms,
            "running": takes.ended.is_none(),
            "variable": takes.variable,
            "levels": takes.levels,
            "repeats": takes.repeats,
            "stop": takes.stop,
            "ended": takes.ended,
        })
    })
}

/// The reading: every value as it stands, defaults stated, what the person
/// set listed, and a previous task's settings disclosed but not applied.
fn reading(
    session: &ResourceRef,
    record: Option<&SettingsRecord>,
    bound: Option<&BoundTask>,
) -> Value {
    let track_set = record.map(|r| r.track.clone()).unwrap_or_default();
    let track = json!({
        "telemetry": track_set.get("telemetry").cloned().unwrap_or(json!(true)),
        "solo": track_set.get("solo").cloned().unwrap_or(json!(false)),
        "set": track_set.keys().collect::<Vec<_>>(),
    });
    let stored = record.and_then(|r| r.task.as_ref());
    let current = stored.filter(|task| bound.is_some_and(|b| b.task_ref == task.task_ref));
    let task = bound.map(|bound| {
        let set = current.map(|task| task.set.clone()).unwrap_or_default();
        let value = |key: &str, default: Value| set.get(key).cloned().unwrap_or(default);
        json!({
            "task_ref": bound.task_ref.as_str(),
            "name": value("name", json!(bound.purpose)),
            "colour": value("colour", Value::Null),
            "active": value("active", json!(true)),
            "register": value("register", json!("plain")),
            "form": value("form", json!("technical")),
            "loop": value("loop", json!(false)),
            "loop_until": value("loop_until", json!("done")),
            "loop_cap": value("loop_cap", Value::Null),
            "launch": value("launch", json!("trigger")),
            "then": value("then", json!("next")),
            "quantize": value("quantize", json!("global")),
            "repeat_variable": repeat_value(&set, "repeat_variable"),
            "repeat_levels": repeat_value(&set, "repeat_levels"),
            "repeat_repeats": repeat_value(&set, "repeat_repeats"),
            "repeat_stop": repeat_value(&set, "repeat_stop"),
            "takes": takes_reading(current.and_then(|task| task.takes.as_ref())),
            "envelope_mode": value("envelope_mode", json!("automation")),
            "envelope_linked": value("envelope_linked", json!(true)),
            // The constellation the task was made of, until the person sets another.
            "source": value("source", bound.source.as_ref().map_or(Value::Null, |source| json!(source))),
            "effort": value("effort", json!([])),
            "autonomy": value("autonomy", json!([])),
            "set": set.keys().collect::<Vec<_>>(),
        })
    });
    let previous = stored
        .filter(|task| current.is_none_or(|current| current.task_ref != task.task_ref))
        .map(|task| json!({"task_ref": task.task_ref.as_str(), "set": task.set}));
    json!({
        "schema": READING_SCHEMA,
        "agent_session": session.as_str(),
        "revision": record.map(|r| r.revision.as_str()),
        "track": track,
        "task": task,
        "previous_task": previous,
        "standing": "The person's settings for this session and its bound task: intent the owners of launch, model and delivery read. Colour null takes the session's colour; an empty envelope inherits the session's value; a null loop cap sets no cap; Repeat's levels stay empty until chosen, and its variable, levels, repeats and stop when do not change while its takes run (a change starts a new set once they end); the source is the constellation the task was made of until another is set, and null when it was made of none.",
    })
}

fn apply(
    session: &ResourceRef,
    current: Option<SettingsRecord>,
    bound: Option<&BoundTask>,
    input: Value,
    expected: Option<&SourceRevision>,
) -> Result<SettingsRecord> {
    let request: SettingsRequest = serde_json::from_value(input)
        .map_err(|e| error(format!("Unreadable session settings request: {e}")))?;
    if current.as_ref().map(|r| &r.revision) != expected {
        return Err(error(
            "Session settings revision conflict; read the current settings and write again",
        ));
    }
    let track_changes = request.track.as_ref().map(validate_track).transpose()?;
    let task_changes = request
        .task
        .as_ref()
        .map(|task| validate_task(&task.set).map(|set| (task.task_ref.clone(), set)))
        .transpose()?;
    if track_changes.as_ref().is_none_or(Map::is_empty)
        && task_changes.as_ref().is_none_or(|(_, set)| set.is_empty())
        && request.takes.is_none()
    {
        return Err(error("Nothing to set"));
    }
    let mut track = current
        .as_ref()
        .map(|r| r.track.clone())
        .unwrap_or_default();
    if let Some(changes) = &track_changes {
        merge(&mut track, changes);
    }
    let mut task = current.as_ref().and_then(|r| r.task.clone());
    if let Some((task_ref, changes)) = task_changes {
        let bound = bound.ok_or_else(|| error("No task is bound to this session"))?;
        if bound.task_ref != task_ref {
            return Err(error(format!(
                "The session's bound task is {}, not {}; read again",
                bound.task_ref.as_str(),
                task_ref.as_str()
            )));
        }
        // A rebound session starts its new task from the defaults.
        let (mut set, takes) = task
            .filter(|task| task.task_ref == task_ref)
            .map(|task| (task.set, task.takes))
            .unwrap_or_default();
        let before = set.clone();
        merge(&mut set, &changes);
        check_envelopes(&mut set)?;
        check_repeat(&set)?;
        check_frozen(takes.as_ref(), &before, &set)?;
        task = Some(TaskSettings {
            task_ref,
            set,
            takes,
        });
    }
    if let Some(act) = request.takes {
        task = Some(takes_act(task, bound, act, now_unix_ms())?);
    }
    Ok(SettingsRecord {
        schema: RECORD_SCHEMA.into(),
        agent_session: session.clone(),
        revision: SourceRevision::parse(format!("session-settings/{}", ulid::Ulid::generate()))?,
        track,
        task,
    })
}

/// "Once the first run starts, these do not change" (DLW.md, The Launch
/// panel): while a set of takes runs, a change to the fields it froze is
/// refused. The same value is no change.
fn check_frozen(
    takes: Option<&TakesSet>,
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> Result<()> {
    let Some(running) = takes.filter(|takes| takes.ended.is_none()) else {
        return Ok(());
    };
    if REPEAT_FIELDS
        .iter()
        .any(|key| repeat_value(before, key) != repeat_value(after, key))
    {
        return Err(error(format!(
            "Once the first run starts, these do not change: the takes {} are running. A change starts a new set of takes once they end.",
            running.trace_ref
        )));
    }
    Ok(())
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// A set of takes starts on the bound task launched with Repeat, freezing
/// its fields as they stand, one set at a time; it ends by its own trace.
fn takes_act(
    task: Option<TaskSettings>,
    bound: Option<&BoundTask>,
    act: TakesRequest,
    now: u64,
) -> Result<TaskSettings> {
    let bound = bound.ok_or_else(|| error("No task is bound to this session"))?;
    let (task_ref, trace_ref) = match &act {
        TakesRequest::Start {
            task_ref,
            trace_ref,
        }
        | TakesRequest::End {
            task_ref,
            trace_ref,
            ..
        } => (task_ref, trace_ref),
    };
    if &bound.task_ref != task_ref {
        return Err(error(format!(
            "The session's bound task is {}, not {}; read again",
            bound.task_ref.as_str(),
            task_ref.as_str()
        )));
    }
    let trace_ref = words("The takes' trace", &json!(trace_ref), 256)?
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let mut task = task
        .filter(|task| &task.task_ref == task_ref)
        .unwrap_or_else(|| TaskSettings {
            task_ref: task_ref.clone(),
            set: Map::new(),
            takes: None,
        });
    let running = task.takes.as_ref().filter(|takes| takes.ended.is_none());
    match act {
        TakesRequest::Start { .. } => {
            if let Some(running) = running {
                return Err(error(format!(
                    "The takes {} are running: a Repeat runs one set of takes at a time",
                    running.trace_ref
                )));
            }
            if task.set.get("launch").and_then(Value::as_str) != Some("repeat") {
                return Err(error(
                    "Only a task launched with Repeat makes takes. Set Launch mode to Repeat first.",
                ));
            }
            task.takes = Some(TakesSet {
                trace_ref,
                started_at_unix_ms: now,
                variable: repeat_value(&task.set, "repeat_variable"),
                levels: repeat_value(&task.set, "repeat_levels"),
                repeats: repeat_value(&task.set, "repeat_repeats"),
                stop: repeat_value(&task.set, "repeat_stop"),
                ended: None,
            });
        }
        TakesRequest::End {
            outcome,
            takes,
            root,
            reason,
            ..
        } => {
            let Some(running) = running.filter(|running| running.trace_ref == trace_ref) else {
                return Err(error(match running {
                    Some(running) => format!(
                        "The takes running are {}, not {trace_ref}",
                        running.trace_ref
                    ),
                    None => format!("No takes are running to end as {trace_ref}"),
                }));
            };
            let outcome = one_of("How the takes ended", &json!(outcome), TAKES_OUTCOMES)?
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let text = |field: &str, value: Option<String>, max: usize| {
                value
                    .map(|value| {
                        words(field, &json!(value), max)
                            .map(|kept| kept.as_str().unwrap_or_default().to_owned())
                    })
                    .transpose()
            };
            let ended = TakesEnd {
                at_unix_ms: now,
                outcome,
                takes,
                root: text("Where the takes are kept", root, 4096)?,
                reason: text("Why the takes ended", reason, 1024)?,
            };
            let mut running = running.clone();
            running.ended = Some(ended);
            task.takes = Some(running);
        }
    }
    Ok(task)
}

impl EncounterService {
    /// `session-space encounter-session-settings --agent-session S`.
    pub fn read_session_settings(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
        Self::new(home.clone())?.require_attached(session)?;
        let record = read_record(home, session)?;
        let bound = bound_task(home, session)?;
        Ok(reading(session, record.as_ref(), bound.as_ref()))
    }

    /// Several sessions' readings at once (several `--agent-session`), in
    /// the order asked: a session no retained SessionSpace holds answers its
    /// refusal in words, the others their readings.
    pub fn read_sessions_settings(home: &AikitHome, sessions: &[ResourceRef]) -> Result<Value> {
        if sessions.len() > MAX_LISTED {
            return Err(error(format!(
                "At most {MAX_LISTED} sessions' settings are read at once"
            )));
        }
        let spaces = SessionSpaceApplicationStore::new(home.clone()).list()?;
        let mut readings = Vec::with_capacity(sessions.len());
        for session in sessions {
            if !spaces
                .iter()
                .any(|space| space.agent_sessions.contains_key(session))
            {
                readings.push(json!({"agent_session": session.as_str(),
                    "refused": "Canonical AgentSession is not attached to any retained SessionSpace"}));
                continue;
            }
            let record = read_record(home, session)?;
            let bound = bound_task(home, session)?;
            readings.push(reading(session, record.as_ref(), bound.as_ref()));
        }
        Ok(json!({"schema": LIST_SCHEMA, "readings": readings}))
    }

    /// The same, with `--request-json` and the revision the change was made
    /// against (absent for a session with no settings yet).
    pub fn write_session_settings(
        home: &AikitHome,
        session: &ResourceRef,
        input: Value,
        expected: Option<&SourceRevision>,
    ) -> Result<Value> {
        let _lock = ContextLock::acquire(
            home,
            &format!(
                "encounter-settings-{}",
                blake3::hash(session.as_str().as_bytes()).to_hex()
            ),
            LockOptions::default(),
        )?;
        Self::new(home.clone())?.require_attached(session)?;
        let bound = bound_task(home, session)?;
        let record = apply(
            session,
            read_record(home, session)?,
            bound.as_ref(),
            input,
            expected,
        )?;
        publish(home, session, &record)?;
        Ok(reading(session, Some(&record), bound.as_ref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/settings-test").unwrap()
    }
    fn bound(task: &str) -> BoundTask {
        BoundTask {
            task_ref: ResourceRef::parse(task).unwrap(),
            purpose: "Write the hand-off fixtures".into(),
            source: None,
        }
    }

    #[test]
    fn a_session_with_no_settings_reads_the_manuals_defaults_and_its_tasks_purpose() {
        let task = bound("task:fixtures");
        let read = reading(&session(), None, Some(&task));
        assert_eq!(read["schema"], READING_SCHEMA);
        assert_eq!(read["revision"], Value::Null);
        assert_eq!(read["track"]["telemetry"], true);
        assert_eq!(read["track"]["solo"], false);
        assert_eq!(read["task"]["name"], "Write the hand-off fixtures");
        // REPLACED (#787): Launch's default was `once`; the manual's Trigger is the same press.
        assert_eq!(read["task"]["launch"], "trigger");
        assert_eq!(read["task"]["then"], "next");
        assert_eq!(read["task"]["loop"], false);
        assert_eq!(read["task"]["loop_until"], "done");
        assert_eq!(read["task"]["loop_cap"], Value::Null);
        assert_eq!(read["task"]["repeat_variable"], "condition");
        assert_eq!(read["task"]["repeat_levels"], json!([]));
        assert_eq!(read["task"]["repeat_repeats"], 3);
        assert_eq!(read["task"]["repeat_stop"], "done");
        assert!(
            read["task"].get("handoff_to").is_none(),
            "Hand off to is retired"
        );
        assert_eq!(read["task"]["quantize"], "global");
        assert_eq!(read["task"]["envelope_mode"], "automation");
        assert_eq!(read["task"]["effort"], json!([]));
        assert_eq!(read["task"]["set"], json!([]));
        let untasked = reading(&session(), None, None);
        assert_eq!(untasked["task"], Value::Null);
    }

    #[test]
    fn a_task_keeps_the_constellation_it_was_made_of_as_its_source() {
        let task = bound("task:fixtures");
        assert_eq!(
            reading(&session(), None, Some(&task))["task"].get("source"),
            Some(&Value::Null),
            "a task made of no constellation reads null"
        );
        let made = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"source": "wiki:anchor:lived-in"}}}),
            None,
        )
        .unwrap();
        let read = reading(&session(), Some(&made), Some(&task));
        assert_eq!(read["task"]["source"], "wiki:anchor:lived-in");
        assert_eq!(read["task"]["set"], json!(["source"]));
        for refused in [
            json!("Lived-in"),
            json!("wiki:"),
            json!(7),
            json!("wiki:anchor:\u{7}"),
        ] {
            let said = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": {"source": refused}}}),
                None,
            )
            .unwrap_err();
            assert!(
                said.to_string()
                    .contains("the wiki ref of the constellation it was made of"),
                "{said}"
            );
        }
    }

    #[test]
    fn a_task_made_of_a_constellation_reads_it_as_its_source_until_another_is_set() {
        let task = BoundTask {
            source: Some("wiki:anchor:lived-in".into()),
            ..bound("task:fixtures")
        };
        let read = reading(&session(), None, Some(&task));
        assert_eq!(read["task"]["source"], "wiki:anchor:lived-in");
        assert_eq!(read["task"]["set"], json!([]), "the person set nothing");
        let other = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"source": "wiki:frame:hand-off"}}}),
            None,
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&other), Some(&task))["task"]["source"],
            "wiki:frame:hand-off"
        );
        let back = apply(
            &session(),
            Some(other.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"source": null}}}),
            Some(&other.revision),
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&back), Some(&task))["task"]["source"],
            "wiki:anchor:lived-in",
            "null returns the task to the constellation it was made of"
        );
    }

    #[test]
    fn a_change_is_kept_under_a_new_revision_and_null_returns_a_field_to_its_default() {
        let task = bound("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&task),
            // REPLACED (#787): the change said launch loop, then handoff and handoff_to; Loop is
            // its own setting now and the handoff is retired.
            json!({"track": {"telemetry": false},
                   "task": {"task_ref": "task:fixtures", "set": {"name": "Fixtures", "launch": "gate", "loop": true,
                            "then": "again", "quantize": "next-tool"}}}),
            None,
        )
        .unwrap();
        let read = reading(&session(), Some(&first), Some(&task));
        assert_eq!(read["track"]["telemetry"], false);
        assert_eq!(read["task"]["name"], "Fixtures");
        assert_eq!(read["task"]["launch"], "gate");
        assert_eq!(read["task"]["loop"], true);
        assert_eq!(read["task"]["then"], "again");
        assert_eq!(read["task"]["quantize"], "next-tool");
        let second = apply(
            &session(),
            Some(first.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"name": null}}}),
            Some(&first.revision),
        )
        .unwrap();
        assert_ne!(second.revision, first.revision);
        let read = reading(&session(), Some(&second), Some(&task));
        assert_eq!(read["task"]["name"], "Write the hand-off fixtures");
        assert_eq!(read["task"]["launch"], "gate", "the other fields stand");
        assert_eq!(read["task"]["loop"], true, "the other fields stand");
        assert_eq!(read["track"]["telemetry"], false, "the track stands");
    }

    #[test]
    fn a_write_against_an_old_revision_or_another_task_is_refused() {
        let task = bound("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&task),
            json!({"track": {"solo": true}}),
            None,
        )
        .unwrap();
        let stale = apply(
            &session(),
            Some(first.clone()),
            Some(&task),
            json!({"track": {"solo": false}}),
            None,
        )
        .unwrap_err();
        assert!(stale.to_string().contains("revision conflict"), "{stale}");
        let other = apply(
            &session(),
            Some(first.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:other", "set": {"launch": "gate"}}}),
            Some(&first.revision),
        )
        .unwrap_err();
        assert!(
            other.to_string().contains("bound task is task:fixtures"),
            "{other}"
        );
        let untasked = apply(
            &session(),
            None,
            None,
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "gate"}}}),
            None,
        )
        .unwrap_err();
        assert!(
            untasked.to_string().contains("No task is bound"),
            "{untasked}"
        );
    }

    #[test]
    fn words_outside_the_manuals_values_are_refused_in_words() {
        let task = bound("task:fixtures");
        // REPLACED (#787): Launch was once · hold · loop and Then next · handoff · stop; a
        // task handed off to its own session was refused, and Hand off to is retired whole.
        for (set, said) in [
            (
                json!({"launch": "twice"}),
                "Launch takes one of trigger, gate, toggle, repeat",
            ),
            (
                json!({"then": "next task"}),
                "Then takes one of next, again, stop",
            ),
            (json!({"quantize": "bar"}), "Quantize takes one of"),
            (
                json!({"register": "loud"}),
                "Register takes one of plain, warm, terse",
            ),
            (json!({"active": "yes"}), "on or off"),
            (json!({"loop": "on"}), "Loop is on or off"),
            (
                json!({"loop_until": "never"}),
                "Loop's stop condition takes one of done, pass",
            ),
            (json!({"loop_cap": 0}), "Loop's cap is a whole number from 1 to"),
            (json!({"loop_cap": 2.5}), "Loop's cap is a whole number from 1 to"),
            (
                json!({"repeat_variable": "temperature"}),
                "Repeat's variable takes one of condition, model, effort, skillset, budget, autonomy",
            ),
            (
                json!({"repeat_repeats": 33}),
                "Repeat's repeats is a whole number from 1 to 32",
            ),
            (
                json!({"repeat_stop": "forever"}),
                "Repeat's stop condition takes one of turns, time, done, pass",
            ),
            (json!({"repeat_levels": []}), "Repeat's levels are 1 to 3"),
            (
                json!({"repeat_levels": ["a", "b", "c", "d"]}),
                "Repeat's levels are 1 to 3",
            ),
            (
                json!({"repeat_levels": ["opus", "opus"]}),
                "Repeat's levels are 1 to 3 distinct values",
            ),
            (json!({"tempo": 120}), "A task has no setting tempo"),
            (json!({"name": "  "}), "Name is text of 1 to 200"),
        ] {
            let refused = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": set}}),
                None,
            )
            .unwrap_err();
            assert!(refused.to_string().contains(said), "{refused} lacks {said}");
        }
        let empty = apply(&session(), None, Some(&task), json!({}), None).unwrap_err();
        assert!(empty.to_string().contains("Nothing to set"));
        let unknown = apply(
            &session(),
            None,
            Some(&task),
            json!({"track": {"arm": true}}),
            None,
        )
        .unwrap_err();
        assert!(unknown.to_string().contains("A track has no setting arm"));
    }

    #[test]
    fn envelopes_hold_words_under_automation_and_steps_under_modulation() {
        let task = bound("task:fixtures");
        let automation = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"effort": [[0, "medium"], [28, "high"]], "autonomy": [[0, "review"]]}}}),
            None,
        )
        .unwrap();
        let read = reading(&session(), Some(&automation), Some(&task));
        assert_eq!(
            read["task"]["effort"],
            json!([[0.0, "medium"], [28.0, "high"]])
        );
        // Switching the mode alone would leave words where steps belong.
        let mixed = apply(
            &session(),
            Some(automation.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"envelope_mode": "modulation"}}}),
            Some(&automation.revision),
        )
        .unwrap_err();
        assert!(
            mixed.to_string().contains("send the points with the mode"),
            "{mixed}"
        );
        let modulation = apply(
            &session(),
            Some(automation.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"envelope_mode": "modulation", "effort": [[0, 0], [30, 1]], "autonomy": [[0, -1]]}}}),
            Some(&automation.revision),
        )
        .unwrap();
        let read = reading(&session(), Some(&modulation), Some(&task));
        assert_eq!(read["task"]["envelope_mode"], "modulation");
        assert_eq!(read["task"]["effort"], json!([[0.0, 0], [30.0, 1]]));
        for bad in [
            json!([[0, 3]]),
            json!([[5, 0], [5, 1]]),
            json!([[-1, 0]]),
            json!([[0]]),
        ] {
            assert!(apply(
                &session(),
                Some(modulation.clone()),
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": {"effort": bad}}}),
                Some(&modulation.revision),
            )
            .is_err());
        }
    }

    #[test]
    fn a_rebound_session_starts_from_the_defaults_and_discloses_the_previous_tasks_settings() {
        let first_task = bound("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&first_task),
            // REPLACED (#787): the first task's launch was loop; Loop is its own setting now.
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "toggle", "loop": true}}}),
            None,
        )
        .unwrap();
        let second_task = bound("task:docs");
        let read = reading(&session(), Some(&first), Some(&second_task));
        assert_eq!(read["task"]["task_ref"], "task:docs");
        assert_eq!(read["task"]["launch"], "trigger");
        assert_eq!(read["task"]["loop"], false);
        assert_eq!(read["previous_task"]["task_ref"], "task:fixtures");
        assert_eq!(read["previous_task"]["set"]["launch"], "toggle");
        assert_eq!(read["previous_task"]["set"]["loop"], true);
        let rebound = apply(
            &session(),
            Some(first.clone()),
            Some(&second_task),
            json!({"task": {"task_ref": "task:docs", "set": {"then": "stop"}}}),
            Some(&first.revision),
        )
        .unwrap();
        let read = reading(&session(), Some(&rebound), Some(&second_task));
        assert_eq!(read["task"]["then"], "stop");
        assert_eq!(
            read["task"]["launch"], "trigger",
            "nothing carries over from the old task"
        );
        assert_eq!(read["task"]["loop"], false);
        assert_eq!(read["previous_task"], Value::Null);
    }

    #[test]
    fn the_record_is_published_with_its_history_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        // Canonical, as the real home is: macOS lends temporary files through /var.
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        let task = bound("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&task),
            json!({"track": {"solo": true}}),
            None,
        )
        .unwrap();
        publish(&home, &session(), &first).unwrap();
        let second = apply(
            &session(),
            read_record(&home, &session()).unwrap(),
            Some(&task),
            json!({"track": {"solo": null}}),
            Some(&first.revision),
        )
        .unwrap();
        publish(&home, &session(), &second).unwrap();
        let back = read_record(&home, &session()).unwrap().unwrap();
        assert_eq!(back.revision, second.revision);
        assert!(back.track.is_empty());
        let history = path(&home, &session()).parent().unwrap().join("history");
        assert_eq!(fs::read_dir(history).unwrap().count(), 1);
    }

    #[test]
    fn the_launch_panel_and_loop_take_and_return_the_manuals_words() {
        let task = bound("task:fixtures");
        for launch in ["trigger", "gate", "toggle", "repeat"] {
            let made = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": {"launch": launch}}}),
                None,
            )
            .unwrap();
            assert_eq!(
                reading(&session(), Some(&made), Some(&task))["task"]["launch"],
                launch
            );
        }
        for then in ["next", "again", "stop"] {
            let made = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": {"then": then}}}),
                None,
            )
            .unwrap();
            assert_eq!(
                reading(&session(), Some(&made), Some(&task))["task"]["then"],
                then
            );
        }
        let looped = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"loop": true, "loop_until": "pass", "loop_cap": 5}}}),
            None,
        )
        .unwrap();
        let read = reading(&session(), Some(&looped), Some(&task));
        assert_eq!(read["task"]["loop"], true);
        assert_eq!(read["task"]["loop_until"], "pass");
        assert_eq!(read["task"]["loop_cap"], 5);
        let repeat = apply(
            &session(),
            Some(looped.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "repeat", "repeat_variable": "model",
                   "repeat_levels": [" opus ", "sonnet", "haiku"], "repeat_repeats": 4, "repeat_stop": "turns", "loop_cap": null}}}),
            Some(&looped.revision),
        )
        .unwrap();
        let read = reading(&session(), Some(&repeat), Some(&task));
        assert_eq!(read["task"]["launch"], "repeat");
        assert_eq!(read["task"]["repeat_variable"], "model");
        assert_eq!(
            read["task"]["repeat_levels"],
            json!(["opus", "sonnet", "haiku"])
        );
        assert_eq!(read["task"]["repeat_repeats"], 4);
        assert_eq!(read["task"]["repeat_stop"], "turns");
        assert_eq!(
            read["task"]["loop_cap"],
            Value::Null,
            "null returns the cap to none"
        );
        assert_eq!(read["task"]["loop_until"], "pass", "the loop stands");
    }

    #[test]
    fn the_handoff_is_retired_naming_the_drag_that_replaces_it() {
        let task = bound("task:fixtures");
        for set in [
            json!({"then": "handoff"}),
            json!({"handoff_to": "agent-session/tests"}),
        ] {
            let refused = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": set}}),
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(
                refused.contains("drag the task onto that session's track"),
                "{refused}"
            );
        }
        for (old, new) in [("once", "trigger"), ("hold", "gate"), ("loop", "Loop")] {
            let refused = apply(
                &session(),
                None,
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": {"launch": old}}}),
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(
                refused.contains(new),
                "{old}: the refusal names {new}: {refused}"
            );
        }
    }

    #[test]
    fn settings_stored_in_the_old_words_read_in_the_manuals_and_a_write_publishes_them() {
        let dir = tempfile::tempdir().unwrap();
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        let task = bound("task:fixtures");
        let stored = |launch: &str, then: &str| {
            let record = json!({"schema": RECORD_SCHEMA, "agent_session": session().as_str(),
                "revision": "session-settings/01OLD", "track": {},
                "task": {"task_ref": "task:fixtures", "set": {"launch": launch, "then": then,
                         "handoff_to": "agent-session/tests", "quantize": "now"}}});
            let target = path(&home, &session());
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, serde_json::to_vec(&record).unwrap()).unwrap();
            read_record(&home, &session()).unwrap().unwrap()
        };
        for (launch, then, now_launch, now_loop, now_then) in [
            ("once", "next", "trigger", None, "next"),
            ("hold", "stop", "gate", Some(true), "stop"),
            ("loop", "handoff", "trigger", Some(true), "stop"),
        ] {
            let record = stored(launch, then);
            let read = reading(&session(), Some(&record), Some(&task));
            assert_eq!(read["task"]["launch"], now_launch, "{launch}");
            assert_eq!(
                read["task"]["loop"],
                now_loop.unwrap_or(false),
                "{launch}: loop"
            );
            assert_eq!(read["task"]["then"], now_then, "{then}");
            assert!(read["task"].get("handoff_to").is_none());
            assert_eq!(read["task"]["quantize"], "now", "the rest stands");
            let mut set: Vec<&str> = read["task"]["set"]
                .as_array()
                .unwrap()
                .iter()
                .map(|key| key.as_str().unwrap())
                .collect();
            set.sort();
            let mut expected = vec!["launch", "quantize", "then"];
            if now_loop.is_some() {
                expected.push("loop");
                expected.sort();
            }
            assert_eq!(set, expected, "{launch}: what the person set");
        }
        // The next write publishes the migrated words; the replaced reading stays in history.
        let record = stored("hold", "handoff");
        let written = apply(
            &session(),
            Some(record.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"loop_cap": 3}}}),
            Some(&record.revision),
        )
        .unwrap();
        publish(&home, &session(), &written).unwrap();
        let file: Value =
            serde_json::from_slice(&fs::read(path(&home, &session())).unwrap()).unwrap();
        assert_eq!(
            file["task"]["set"],
            json!({"launch": "gate", "loop": true, "then": "stop", "quantize": "now", "loop_cap": 3})
        );
        let history = path(&home, &session()).parent().unwrap().join("history");
        assert_eq!(fs::read_dir(history).unwrap().count(), 1);
    }

    #[test]
    fn repeat_levels_are_the_variables_words_where_aikit_keeps_them() {
        let task = bound("task:fixtures");
        let effort = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_variable": "effort", "repeat_levels": ["low", "max"]}}}),
            None,
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&effort), Some(&task))["task"]["repeat_levels"],
            json!(["low", "max"])
        );
        let wrong = apply(
            &session(),
            None,
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_variable": "autonomy", "repeat_levels": ["opus"]}}}),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            wrong.contains("each of Repeat's levels is one of inherit, suggest, review, auto"),
            "{wrong}"
        );
        // Switching the variable alone would leave Effort's words as Autonomy's levels.
        let moved = apply(
            &session(),
            Some(effort.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_variable": "autonomy"}}}),
            Some(&effort.revision),
        )
        .unwrap_err()
        .to_string();
        assert!(
            moved.contains("send the levels with the variable"),
            "{moved}"
        );
        // Condition, Model, Skillset and Budget name their own owners' values: AIKit keeps no copy.
        let condition = apply(
            &session(),
            Some(effort.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_variable": "condition", "repeat_levels": ["classic", "ql-direct"]}}}),
            Some(&effort.revision),
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&condition), Some(&task))["task"]["repeat_variable"],
            "condition"
        );
    }

    /// A task launched with Repeat, its fields chosen, and its takes started.
    fn repeating(task: &BoundTask) -> SettingsRecord {
        let set = apply(
            &session(),
            None,
            Some(task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "repeat",
                "repeat_levels": ["classic", "ql-direct"], "repeat_repeats": 4}}}),
            None,
        )
        .unwrap();
        apply(
            &session(),
            Some(set.clone()),
            Some(task),
            json!({"takes": {"start": {"task_ref": "task:fixtures", "trace_ref": "takes:t1"}}}),
            Some(&set.revision),
        )
        .unwrap()
    }

    #[test]
    fn once_the_first_run_starts_repeats_fields_do_not_change() {
        let task = bound("task:fixtures");
        let running = repeating(&task);
        let read = reading(&session(), Some(&running), Some(&task));
        assert_eq!(read["task"]["takes"]["trace_ref"], "takes:t1");
        assert_eq!(read["task"]["takes"]["running"], true);
        // The fields the takes run with, as they stood when the first run started (defaults stated).
        assert_eq!(read["task"]["takes"]["variable"], "condition");
        assert_eq!(
            read["task"]["takes"]["levels"],
            json!(["classic", "ql-direct"])
        );
        assert_eq!(read["task"]["takes"]["repeats"], 4);
        assert_eq!(read["task"]["takes"]["stop"], "done");
        // "Once the first run starts, these do not change" (DLW.md, The Launch panel).
        for change in [
            json!({"repeat_levels": ["classic"]}),
            json!({"repeat_variable": "effort", "repeat_levels": ["low"]}),
            json!({"repeat_repeats": 5}),
            json!({"repeat_stop": "pass"}),
            json!({"repeat_repeats": null}),
        ] {
            let refused = apply(
                &session(),
                Some(running.clone()),
                Some(&task),
                json!({"task": {"task_ref": "task:fixtures", "set": change}}),
                Some(&running.revision),
            )
            .unwrap_err()
            .to_string();
            assert!(
                refused.contains("Once the first run starts, these do not change")
                    && refused.contains("takes:t1"),
                "{change}: {refused}"
            );
        }
        // The same value is no change; the task's other settings still change.
        let same = apply(
            &session(),
            Some(running.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_repeats": 4, "name": "Fixtures"}}}),
            Some(&running.revision),
        )
        .unwrap();
        let read = reading(&session(), Some(&same), Some(&task));
        assert_eq!(read["task"]["name"], "Fixtures");
        assert_eq!(read["task"]["takes"]["running"], true, "the takes stand");
        // Once they end, a change starts a new set of takes.
        let ended = apply(
            &session(),
            Some(same.clone()),
            Some(&task),
            json!({"takes": {"end": {"task_ref": "task:fixtures", "trace_ref": "takes:t1", "outcome": "made",
                "takes": 8, "root": "/takes/t1"}}}),
            Some(&same.revision),
        )
        .unwrap();
        let read = reading(&session(), Some(&ended), Some(&task));
        assert_eq!(read["task"]["takes"]["running"], false);
        assert_eq!(read["task"]["takes"]["ended"]["outcome"], "made");
        assert_eq!(read["task"]["takes"]["ended"]["takes"], 8);
        assert_eq!(read["task"]["takes"]["ended"]["root"], "/takes/t1");
        let changed = apply(
            &session(),
            Some(ended.clone()),
            Some(&task),
            json!({"task": {"task_ref": "task:fixtures", "set": {"repeat_repeats": 5}}}),
            Some(&ended.revision),
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&changed), Some(&task))["task"]["repeat_repeats"],
            5
        );
    }

    #[test]
    fn takes_start_on_a_repeat_launch_one_set_at_a_time_and_end_by_their_own_trace() {
        let task = bound("task:fixtures");
        let trigger = apply(
            &session(),
            None,
            Some(&task),
            json!({"takes": {"start": {"task_ref": "task:fixtures", "trace_ref": "takes:t0"}}}),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(trigger.contains("Set Launch mode to Repeat"), "{trigger}");
        let running = repeating(&task);
        let second = apply(
            &session(),
            Some(running.clone()),
            Some(&task),
            json!({"takes": {"start": {"task_ref": "task:fixtures", "trace_ref": "takes:t2"}}}),
            Some(&running.revision),
        )
        .unwrap_err()
        .to_string();
        assert!(second.contains("takes:t1 are running"), "{second}");
        let other = apply(
            &session(),
            Some(running.clone()),
            Some(&task),
            json!({"takes": {"end": {"task_ref": "task:fixtures", "trace_ref": "takes:t9", "outcome": "made"}}}),
            Some(&running.revision),
        )
        .unwrap_err()
        .to_string();
        assert!(other.contains("takes:t1"), "{other}");
        let word = apply(
            &session(),
            Some(running.clone()),
            Some(&task),
            json!({"takes": {"end": {"task_ref": "task:fixtures", "trace_ref": "takes:t1", "outcome": "done"}}}),
            Some(&running.revision),
        )
        .unwrap_err()
        .to_string();
        assert!(word.contains("made, failed, stopped"), "{word}");
        // A rebound session's new task starts with no takes.
        let rebound = reading(&session(), Some(&running), Some(&bound("task:docs")));
        assert_eq!(rebound["task"]["takes"], Value::Null);
        // A stopped set ends with its reason, and another may start.
        let stopped = apply(
            &session(),
            Some(running.clone()),
            Some(&task),
            json!({"takes": {"end": {"task_ref": "task:fixtures", "trace_ref": "takes:t1", "outcome": "stopped",
                "reason": "The person stopped the takes"}}}),
            Some(&running.revision),
        )
        .unwrap();
        let next = apply(
            &session(),
            Some(stopped.clone()),
            Some(&task),
            json!({"takes": {"start": {"task_ref": "task:fixtures", "trace_ref": "takes:t2"}}}),
            Some(&stopped.revision),
        )
        .unwrap();
        assert_eq!(
            reading(&session(), Some(&next), Some(&task))["task"]["takes"]["trace_ref"],
            "takes:t2"
        );
    }
}
