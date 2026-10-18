//! An agent session's own settings, beside its bound task: how its track
//! plays (Telemetry, Solo) and how its task launches, what follows it and how
//! the agent works in it (name, colour, activator, output style, Launch, Then,
//! Hand off to, Quantize, and the Effort and Autonomy envelopes). The task
//! record stays the admission's; these settings are the person's, kept under
//! their own revision with every replaced reading in history. Task settings
//! bind to the task they were written for (`task_ref`): a rebound session
//! starts from the defaults and discloses the previous task's settings.
//! Settings are intent. Nothing here selects a model, starts a turn or moves a
//! task; the owners of those acts read these settings.
use super::{error, read as read_task_record};
use crate::encounter_service::EncounterService;
use aikit_core::{ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{fs, io::Write, path::PathBuf};

pub const READING_SCHEMA: &str = "aikit.encounter-session-settings/v1";
const RECORD_SCHEMA: &str = "aikit.encounter-session-settings-record/v1";
const MAX_POINTS: usize = 64;
const MAX_SECONDS: f64 = 7.0 * 24.0 * 3600.0;

const REGISTERS: &[&str] = &["plain", "warm", "terse"];
const FORMS: &[&str] = &["technical", "narrative", "list"];
const LAUNCHES: &[&str] = &["once", "hold", "loop"];
const THENS: &[&str] = &["next", "handoff", "stop"];
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
}

/// The person's change. `null` returns a field to its default.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsRequest {
    #[serde(default)]
    track: Option<Map<String, Value>>,
    #[serde(default)]
    task: Option<TaskRequest>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskRequest {
    task_ref: ResourceRef,
    set: Map<String, Value>,
}

/// The bound task as the settings see it: its identity and its purpose (the
/// name a task has before the person names it).
#[derive(Debug, Clone, PartialEq)]
struct BoundTask {
    task_ref: ResourceRef,
    purpose: String,
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
    Ok(Some(record))
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
/// checked against the merged mode afterwards.
fn validate_task(
    session: &ResourceRef,
    changes: &Map<String, Value>,
) -> Result<Map<String, Value>> {
    let mut kept = Map::new();
    for (key, value) in changes {
        let value = match (key.as_str(), value) {
            (_, Value::Null) => Value::Null,
            ("name", value) => words("Name", value, 200)?,
            ("colour", value) => words("Colour", value, 64)?,
            ("active", value) => flag("The clip activator", value)?,
            ("register", value) => one_of("Register", value, REGISTERS)?,
            ("form", value) => one_of("Form", value, FORMS)?,
            ("launch", value) => one_of("Launch", value, LAUNCHES)?,
            ("then", value) => one_of("Then", value, THENS)?,
            ("handoff_to", value) => {
                let target = value
                    .as_str()
                    .map(ResourceRef::parse)
                    .transpose()?
                    .ok_or_else(|| error("Hand off to names an agent session"))?;
                if &target == session {
                    return Err(error("A task cannot hand off to its own session"));
                }
                json!(target.as_str())
            }
            ("quantize", value) => one_of("Quantize", value, QUANTIZES)?,
            ("envelope_mode", value) => one_of("The envelope mode", value, MODES)?,
            ("envelope_linked", value) => flag("The envelope link", value)?,
            // Checked against the merged mode once every change is in.
            ("effort", value) | ("autonomy", value) => value.clone(),
            (other, _) => {
                return Err(error(format!(
                    "A task has no setting {other}; it takes name, colour, active, register, form, launch, then, handoff_to, quantize, envelope_mode, envelope_linked, effort and autonomy"
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
    }))
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
            "launch": value("launch", json!("once")),
            "then": value("then", json!("next")),
            "handoff_to": value("handoff_to", Value::Null),
            "quantize": value("quantize", json!("global")),
            "envelope_mode": value("envelope_mode", json!("automation")),
            "envelope_linked": value("envelope_linked", json!(true)),
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
        "standing": "The person's settings for this session and its bound task: intent the owners of launch, model and delivery read. Colour null takes the session's colour; an empty envelope inherits the session's value.",
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
        .map(|task| validate_task(session, &task.set).map(|set| (task.task_ref.clone(), set)))
        .transpose()?;
    if track_changes.as_ref().is_none_or(Map::is_empty)
        && task_changes.as_ref().is_none_or(|(_, set)| set.is_empty())
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
        let mut set = task
            .filter(|task| task.task_ref == task_ref)
            .map(|task| task.set)
            .unwrap_or_default();
        merge(&mut set, &changes);
        check_envelopes(&mut set)?;
        task = Some(TaskSettings { task_ref, set });
    }
    Ok(SettingsRecord {
        schema: RECORD_SCHEMA.into(),
        agent_session: session.clone(),
        revision: SourceRevision::parse(format!("session-settings/{}", ulid::Ulid::generate()))?,
        track,
        task,
    })
}

impl EncounterService {
    /// `session-space encounter-session-settings --agent-session S`.
    pub fn read_session_settings(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
        Self::new(home.clone())?.require_attached(session)?;
        let record = read_record(home, session)?;
        let bound = bound_task(home, session)?;
        Ok(reading(session, record.as_ref(), bound.as_ref()))
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
        assert_eq!(read["task"]["launch"], "once");
        assert_eq!(read["task"]["then"], "next");
        assert_eq!(read["task"]["quantize"], "global");
        assert_eq!(read["task"]["envelope_mode"], "automation");
        assert_eq!(read["task"]["effort"], json!([]));
        assert_eq!(read["task"]["set"], json!([]));
        let untasked = reading(&session(), None, None);
        assert_eq!(untasked["task"], Value::Null);
    }

    #[test]
    fn a_change_is_kept_under_a_new_revision_and_null_returns_a_field_to_its_default() {
        let task = bound("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&task),
            json!({"track": {"telemetry": false},
                   "task": {"task_ref": "task:fixtures", "set": {"name": "Fixtures", "launch": "loop", "then": "handoff",
                            "handoff_to": "agent-session/tests", "quantize": "next-tool"}}}),
            None,
        )
        .unwrap();
        let read = reading(&session(), Some(&first), Some(&task));
        assert_eq!(read["track"]["telemetry"], false);
        assert_eq!(read["task"]["name"], "Fixtures");
        assert_eq!(read["task"]["launch"], "loop");
        assert_eq!(read["task"]["handoff_to"], "agent-session/tests");
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
        assert_eq!(read["task"]["launch"], "loop", "the other fields stand");
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
            json!({"task": {"task_ref": "task:other", "set": {"launch": "hold"}}}),
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
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "hold"}}}),
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
        for (set, said) in [
            (
                json!({"launch": "twice"}),
                "Launch takes one of once, hold, loop",
            ),
            (
                json!({"then": "next task"}),
                "Then takes one of next, handoff, stop",
            ),
            (json!({"quantize": "bar"}), "Quantize takes one of"),
            (
                json!({"register": "loud"}),
                "Register takes one of plain, warm, terse",
            ),
            (json!({"active": "yes"}), "on or off"),
            (
                json!({"handoff_to": "agent-session/settings-test"}),
                "cannot hand off to its own session",
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
            json!({"task": {"task_ref": "task:fixtures", "set": {"launch": "loop"}}}),
            None,
        )
        .unwrap();
        let second_task = bound("task:docs");
        let read = reading(&session(), Some(&first), Some(&second_task));
        assert_eq!(read["task"]["task_ref"], "task:docs");
        assert_eq!(read["task"]["launch"], "once");
        assert_eq!(read["previous_task"]["task_ref"], "task:fixtures");
        assert_eq!(read["previous_task"]["set"]["launch"], "loop");
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
            read["task"]["launch"], "once",
            "nothing carries over from the old task"
        );
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
}
