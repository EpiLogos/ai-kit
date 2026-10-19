//! The attempts a session's task serves. A task is dispatched by the Software
//! Factory as an attempt of a Run's workflow unit; the attempt names the
//! session (`disposition.body.agent_session_ref`), but nothing named the
//! attempt from the session's side. This record is that naming: Factory writes
//! it once the attempt is bound to its dispatch, the DLW shell reads it for
//! every track at once and lays each attempt as a take under the session.
//! The task record stays the admission's and the attempts stay Factory's; this
//! sidecar only says which attempt (and in which Factory state) the session's
//! bound task serves, under its own revision with every replaced reading in
//! history. It starts nothing, lands nothing and chooses nothing.
use super::{error, read as read_task_record};
use crate::encounter_service::EncounterService;
use aikit_core::{ResourceRef, Result, SourceRevision};
use aikit_store::{AikitHome, ContextLock, LockOptions, SessionSpaceApplicationStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Write, path::PathBuf};

pub const READING_SCHEMA: &str = "aikit.encounter-task-attempts/v1";
pub const LIST_SCHEMA: &str = "aikit.encounter-task-attempts-list/v1";
const RECORD_SCHEMA: &str = "aikit.encounter-task-attempts-record/v1";
const MAX_LISTED: usize = 256;
const MAX_ATTEMPTS: usize = 512;
const MAX_TEXT: usize = 1024;

/// One attempt a task serves, as its dispatcher recorded it. The Factory
/// state is a path on this machine; the refs are Factory's own, carried
/// verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ServedAttempt {
    /// The task this attempt was dispatched on (the bound task's `task_ref`).
    task_ref: ResourceRef,
    attempt_ref: String,
    run_ref: String,
    /// Factory's native state holding the Run (`factory.attempt-state/v1`).
    state_path: PathBuf,
    /// The workflow unit the attempt works, as Factory names it.
    #[serde(default)]
    workflow_unit: Value,
    #[serde(default)]
    execution_ref: Option<String>,
    #[serde(default)]
    delivery_ref: Option<String>,
    /// Who recorded it (the dispatching owner's caller).
    recorded_by: String,
    recorded_at_unix_ms: u64,
}

/// The stored record: every attempt this session's tasks have served.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptsRecord {
    schema: String,
    agent_session: ResourceRef,
    revision: SourceRevision,
    attempts: Vec<ServedAttempt>,
}

/// The dispatcher's request: one attempt the bound task now serves. The
/// attempt's `task_ref` is the task it was dispatched on, so a record can
/// only be written against the task that stands.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptsRequest {
    attempt: AttemptRequest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AttemptRequest {
    task_ref: ResourceRef,
    attempt_ref: String,
    run_ref: String,
    state_path: PathBuf,
    #[serde(default)]
    workflow_unit: Value,
    #[serde(default)]
    execution_ref: Option<String>,
    #[serde(default)]
    delivery_ref: Option<String>,
    recorded_by: String,
}

fn path(home: &AikitHome, session: &ResourceRef) -> PathBuf {
    home.state()
        .join("encounter-tasks")
        .join("attempts")
        .join(format!(
            "{}.json",
            blake3::hash(session.as_str().as_bytes()).to_hex()
        ))
}

fn read_record(home: &AikitHome, session: &ResourceRef) -> Result<Option<AttemptsRecord>> {
    let path = path(home, session);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        Ok(meta) if !meta.is_file() || meta.len() > 4 * 1024 * 1024 => {
            return Err(error("Task attempts must be a bounded native file"))
        }
        Ok(_) => {}
    }
    if path.canonicalize().map_err(error)? != path {
        return Err(error("Redirected task attempts"));
    }
    let record: AttemptsRecord = serde_json::from_slice(&fs::read(&path).map_err(error)?)
        .map_err(|e| error(format!("Unreadable task attempts: {e}")))?;
    if record.schema != RECORD_SCHEMA || &record.agent_session != session {
        return Err(error("Task attempts belong to another schema or session"));
    }
    Ok(Some(record))
}

/// Publish by atomic replacement, keeping the replaced reading in history.
fn publish(home: &AikitHome, session: &ResourceRef, record: &AttemptsRecord) -> Result<()> {
    let target = path(home, session);
    let parent = target.parent().expect("attempts parent");
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
                    return Err(error("Task attempts history conflict"));
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

fn text(field: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > MAX_TEXT || value.chars().any(char::is_control) {
        return Err(error(format!(
            "{field} is text of 1 to {MAX_TEXT} characters, without control characters"
        )));
    }
    Ok(value.to_owned())
}

fn optional_text(field: &str, value: Option<&String>) -> Result<Option<String>> {
    value.map(|value| text(field, value)).transpose()
}

fn bound_task(home: &AikitHome, session: &ResourceRef) -> Result<Option<ResourceRef>> {
    Ok(read_task_record(home, session)?.map(|record| record.request.central.task_ref.clone()))
}

fn served(request: AttemptRequest, now_unix_ms: u64) -> Result<ServedAttempt> {
    if !request.state_path.is_absolute() {
        return Err(error("The Factory state of an attempt is an absolute path"));
    }
    let workflow_unit = if request.workflow_unit.is_null()
        || request.workflow_unit.is_object()
        || request.workflow_unit.is_string()
    {
        request.workflow_unit
    } else {
        return Err(error(
            "The workflow unit is Factory's own ref (an object or text)",
        ));
    };
    if serde_json::to_vec(&workflow_unit).map_err(error)?.len() > 4096 {
        return Err(error("The workflow unit ref is too large"));
    }
    Ok(ServedAttempt {
        task_ref: request.task_ref,
        attempt_ref: text("The attempt ref", &request.attempt_ref)?,
        run_ref: text("The run ref", &request.run_ref)?,
        state_path: request.state_path,
        workflow_unit,
        execution_ref: optional_text("The execution ref", request.execution_ref.as_ref())?,
        delivery_ref: optional_text("The delivery ref", request.delivery_ref.as_ref())?,
        recorded_by: text("Recorded by", &request.recorded_by)?,
        recorded_at_unix_ms: now_unix_ms,
    })
}

/// The same attempt as an earlier record, whatever the clock said when it was
/// recorded: a replay of the dispatcher's write changes nothing.
fn same_attempt(a: &ServedAttempt, b: &ServedAttempt) -> bool {
    a.task_ref == b.task_ref
        && a.attempt_ref == b.attempt_ref
        && a.run_ref == b.run_ref
        && a.state_path == b.state_path
        && a.workflow_unit == b.workflow_unit
        && a.execution_ref == b.execution_ref
        && a.delivery_ref == b.delivery_ref
        && a.recorded_by == b.recorded_by
}

/// The reading: the attempts the bound task serves, and the ones its earlier
/// tasks served, disclosed but not applied.
fn reading(
    session: &ResourceRef,
    record: Option<&AttemptsRecord>,
    bound: Option<&ResourceRef>,
) -> Value {
    let all = record.map(|r| r.attempts.as_slice()).unwrap_or_default();
    let (serving, earlier): (Vec<_>, Vec<_>) = all
        .iter()
        .partition(|attempt| bound.is_some_and(|task| *task == attempt.task_ref));
    json!({
        "schema": READING_SCHEMA,
        "agent_session": session.as_str(),
        "revision": record.map(|r| r.revision.as_str()),
        "task_ref": bound.map(ResourceRef::as_str),
        "attempts": serving,
        "earlier": earlier,
        "standing": "The attempts the session's bound task serves, as the Software Factory that dispatched them recorded them. Factory owns the attempts: read each in its Factory state. Nothing here starts, lands or chooses an attempt.",
    })
}

/// The change: one attempt joins the record, or is already there. The first
/// answer of `None` is a replay that changes nothing.
fn apply(
    session: &ResourceRef,
    current: Option<AttemptsRecord>,
    bound: Option<&ResourceRef>,
    input: Value,
    expected: Option<&SourceRevision>,
    now_unix_ms: u64,
) -> Result<Option<AttemptsRecord>> {
    let request: AttemptsRequest = serde_json::from_value(input)
        .map_err(|e| error(format!("Unreadable task attempts request: {e}")))?;
    let bound = bound.ok_or_else(|| error("No task is bound to this session"))?;
    if &request.attempt.task_ref != bound {
        return Err(error(format!(
            "The session's bound task is {}, not {}; read again",
            bound.as_str(),
            request.attempt.task_ref.as_str()
        )));
    }
    let attempt = served(request.attempt, now_unix_ms)?;
    let mut attempts = current
        .as_ref()
        .map(|r| r.attempts.clone())
        .unwrap_or_default();
    // A replay of a recorded attempt is the dispatcher retrying its write; it
    // needs no revision, and a different record under the same attempt is a
    // conflict, never a replacement.
    if let Some(known) = attempts.iter().find(|known| {
        known.task_ref == attempt.task_ref && known.attempt_ref == attempt.attempt_ref
    }) {
        return if same_attempt(known, &attempt) {
            Ok(None)
        } else {
            Err(error(format!(
                "The attempt {} is already recorded for this task with different content",
                attempt.attempt_ref
            )))
        };
    }
    if current.as_ref().map(|r| &r.revision) != expected {
        return Err(error(
            "Task attempts revision conflict; read the current attempts and write again",
        ));
    }
    if attempts.len() >= MAX_ATTEMPTS {
        return Err(error(format!(
            "A session records at most {MAX_ATTEMPTS} attempts"
        )));
    }
    attempts.push(attempt);
    Ok(Some(AttemptsRecord {
        schema: RECORD_SCHEMA.into(),
        agent_session: session.clone(),
        revision: SourceRevision::parse(format!("task-attempts/{}", ulid::Ulid::generate()))?,
        attempts,
    }))
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

impl EncounterService {
    /// `session-space encounter-task-attempts --agent-session S`.
    pub fn read_task_attempts(home: &AikitHome, session: &ResourceRef) -> Result<Value> {
        Self::new(home.clone())?.require_attached(session)?;
        let record = read_record(home, session)?;
        let bound = bound_task(home, session)?;
        Ok(reading(session, record.as_ref(), bound.as_ref()))
    }

    /// Several sessions' attempts at once (several `--agent-session`), in the
    /// order asked: a session no retained SessionSpace holds answers its
    /// refusal in words, the others their readings.
    pub fn read_sessions_task_attempts(
        home: &AikitHome,
        sessions: &[ResourceRef],
    ) -> Result<Value> {
        if sessions.len() > MAX_LISTED {
            return Err(error(format!(
                "At most {MAX_LISTED} sessions' attempts are read at once"
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

    /// The same, with `--request-json`: the dispatcher records one attempt the
    /// bound task now serves (against the revision that stands, absent for a
    /// session with none yet). A replay answers the standing reading.
    pub fn write_task_attempts(
        home: &AikitHome,
        session: &ResourceRef,
        input: Value,
        expected: Option<&SourceRevision>,
    ) -> Result<Value> {
        let _lock = ContextLock::acquire(
            home,
            &format!(
                "encounter-attempts-{}",
                blake3::hash(session.as_str().as_bytes()).to_hex()
            ),
            LockOptions::default(),
        )?;
        Self::new(home.clone())?.require_attached(session)?;
        let bound = bound_task(home, session)?;
        let current = read_record(home, session)?;
        match apply(
            session,
            current.clone(),
            bound.as_ref(),
            input,
            expected,
            now_unix_ms(),
        )? {
            Some(record) => {
                publish(home, session, &record)?;
                Ok(reading(session, Some(&record), bound.as_ref()))
            }
            None => Ok(reading(session, current.as_ref(), bound.as_ref())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/attempts-test").unwrap()
    }
    fn task(name: &str) -> ResourceRef {
        ResourceRef::parse(name).unwrap()
    }
    fn request(task_ref: &str, attempt_ref: &str) -> Value {
        json!({"attempt": {
            "task_ref": task_ref, "attempt_ref": attempt_ref, "run_ref": "run:build",
            "state_path": "/factory/state.json",
            "workflow_unit": {"workflow_key": "build", "unit": "write"},
            "execution_ref": "execution:one", "delivery_ref": "delivery:one",
            "recorded_by": "factory",
        }})
    }

    #[test]
    fn a_session_with_no_record_reads_no_attempts_and_the_bound_task() {
        let bound = task("task:fixtures");
        let read = reading(&session(), None, Some(&bound));
        assert_eq!(read["schema"], READING_SCHEMA);
        assert_eq!(read["revision"], Value::Null);
        assert_eq!(read["task_ref"], "task:fixtures");
        assert_eq!(read["attempts"], json!([]));
        assert_eq!(read["earlier"], json!([]));
        let untasked = reading(&session(), None, None);
        assert_eq!(untasked["task_ref"], Value::Null);
    }

    #[test]
    fn a_recorded_attempt_is_read_under_a_new_revision() {
        let bound = task("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&bound),
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap()
        .unwrap();
        let read = reading(&session(), Some(&first), Some(&bound));
        assert_eq!(read["attempts"][0]["attempt_ref"], "attempt:one");
        assert_eq!(read["attempts"][0]["run_ref"], "run:build");
        assert_eq!(read["attempts"][0]["state_path"], "/factory/state.json");
        assert_eq!(read["attempts"][0]["workflow_unit"]["unit"], "write");
        assert_eq!(read["attempts"][0]["recorded_by"], "factory");
        assert_eq!(read["attempts"][0]["recorded_at_unix_ms"], 10);
        let second = apply(
            &session(),
            Some(first.clone()),
            Some(&bound),
            request("task:fixtures", "attempt:two"),
            Some(&first.revision),
            20,
        )
        .unwrap()
        .unwrap();
        assert_ne!(second.revision, first.revision);
        let read = reading(&session(), Some(&second), Some(&bound));
        assert_eq!(read["attempts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_replay_changes_nothing_and_a_different_record_under_the_same_attempt_is_refused() {
        let bound = task("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&bound),
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap()
        .unwrap();
        // The dispatcher retries its write against a stale or absent revision.
        let replay = apply(
            &session(),
            Some(first.clone()),
            Some(&bound),
            request("task:fixtures", "attempt:one"),
            None,
            99,
        )
        .unwrap();
        assert!(replay.is_none(), "an exact replay is no new revision");
        let mut changed = request("task:fixtures", "attempt:one");
        changed["attempt"]["run_ref"] = json!("run:other");
        let refused = apply(
            &session(),
            Some(first.clone()),
            Some(&bound),
            changed,
            Some(&first.revision),
            99,
        )
        .unwrap_err();
        assert!(
            refused.to_string().contains("already recorded"),
            "{refused}"
        );
    }

    #[test]
    fn a_write_against_an_old_revision_another_task_or_no_task_is_refused() {
        let bound = task("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&bound),
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap()
        .unwrap();
        let stale = apply(
            &session(),
            Some(first.clone()),
            Some(&bound),
            request("task:fixtures", "attempt:two"),
            None,
            20,
        )
        .unwrap_err();
        assert!(stale.to_string().contains("revision conflict"), "{stale}");
        let other = apply(
            &session(),
            Some(first.clone()),
            Some(&bound),
            request("task:other", "attempt:two"),
            Some(&first.revision),
            20,
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
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap_err();
        assert!(
            untasked.to_string().contains("No task is bound"),
            "{untasked}"
        );
    }

    #[test]
    fn a_request_outside_the_contract_is_refused_in_words() {
        let bound = task("task:fixtures");
        for (change, said) in [
            (
                json!({"state_path": "relative/state.json"}),
                "absolute path",
            ),
            (json!({"attempt_ref": "  "}), "The attempt ref is text"),
            (json!({"run_ref": ""}), "The run ref is text"),
            (json!({"recorded_by": "bad\nname"}), "Recorded by is text"),
            (json!({"workflow_unit": 7}), "Factory's own ref"),
        ] {
            let mut input = request("task:fixtures", "attempt:one");
            for (key, value) in change.as_object().unwrap() {
                input["attempt"][key] = value.clone();
            }
            let refused = apply(&session(), None, Some(&bound), input, None, 10).unwrap_err();
            assert!(refused.to_string().contains(said), "{refused} lacks {said}");
        }
        let mut unknown = request("task:fixtures", "attempt:one");
        unknown["attempt"]["model"] = json!("sonnet");
        let refused = apply(&session(), None, Some(&bound), unknown, None, 10).unwrap_err();
        assert!(
            refused
                .to_string()
                .contains("Unreadable task attempts request"),
            "{refused}"
        );
    }

    #[test]
    fn a_rebound_session_reads_the_new_tasks_attempts_and_discloses_the_earlier_ones() {
        let first_task = task("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&first_task),
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap()
        .unwrap();
        let second_task = task("task:docs");
        let read = reading(&session(), Some(&first), Some(&second_task));
        assert_eq!(read["task_ref"], "task:docs");
        assert_eq!(read["attempts"], json!([]));
        assert_eq!(read["earlier"][0]["attempt_ref"], "attempt:one");
        let rebound = apply(
            &session(),
            Some(first.clone()),
            Some(&second_task),
            request("task:docs", "attempt:one"),
            Some(&first.revision),
            30,
        )
        .unwrap()
        .unwrap();
        let read = reading(&session(), Some(&rebound), Some(&second_task));
        assert_eq!(
            read["attempts"].as_array().unwrap().len(),
            1,
            "the same attempt ref under another task is its own attempt"
        );
        assert_eq!(read["earlier"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn the_record_is_published_with_its_history_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        // Canonical, as the real home is: macOS lends temporary files through /var.
        let home = AikitHome::at(dir.path().canonicalize().unwrap().join("home"));
        let bound = task("task:fixtures");
        let first = apply(
            &session(),
            None,
            Some(&bound),
            request("task:fixtures", "attempt:one"),
            None,
            10,
        )
        .unwrap()
        .unwrap();
        publish(&home, &session(), &first).unwrap();
        let second = apply(
            &session(),
            read_record(&home, &session()).unwrap(),
            Some(&bound),
            request("task:fixtures", "attempt:two"),
            Some(&first.revision),
            20,
        )
        .unwrap()
        .unwrap();
        publish(&home, &session(), &second).unwrap();
        let back = read_record(&home, &session()).unwrap().unwrap();
        assert_eq!(back.revision, second.revision);
        assert_eq!(back.attempts.len(), 2);
        let history = path(&home, &session()).parent().unwrap().join("history");
        assert_eq!(fs::read_dir(history).unwrap().count(), 1);
    }
}
