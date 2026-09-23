//! `aikit whoami`: the joined World inhabitation reading
//! (`aikit.inhabitation-reading/v1`, O:I `WORLD-INHABITATION-V1` §4).
//!
//! AIKit owns none of the facts joined here. Central answers the Local World,
//! Project World, Position definitions, the NOW horizon and native authority;
//! Actuation answers occupancy and tenure; Factory answers work custody and the
//! current work; AIKit adds only what it owns — SessionSpace, working Surface,
//! body composition and the prepared-context/World projections in Redis.
//!
//! Every owner is reached through one command seam ([`Owners`]) with a hard
//! per-call bound and an overall deadline, and every answer lands in a facet
//! as `present | absent | ambiguous | unavailable | not-attempted`. An owner
//! verb that is missing, hangs or refuses becomes `unavailable` carrying the
//! exact command and its error; nothing is inferred to fill the hole.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use aikit_adapters::runner::{CommandRunner, Output};
use aikit_core::inhabitation::{
    Facet, FacetState, HotProvenance, InhabitationFacets, InhabitationIdentity,
    InhabitationReading, OwnerCallRecord, ReadingDepth, ResolvedBy, INHABITATION_READING_SCHEMA,
};
use aikit_core::{AikitError, Result};
use aikit_store::home::AikitHome;
use aikit_store::now_context::{WorldProjection, WORLD_PROJECTION_SCHEMA};
use serde_json::{json, Value};

pub const POSITION_VAR: &str = "OI_POSITION_REF";
pub const GENERATION_VAR: &str = "OI_OCCUPANT_GENERATION";
pub const AGENT_SESSION_VAR: &str = "AIKIT_SESSION_ID";
pub const REDIS_CONFIG_VAR: &str = "AIKIT_WORLD_REDIS_CONFIG";
pub const OCCUPANCY_STORE_VAR: &str = "ACTUATION_OCCUPANCY_STORE";
pub const NATIVE_TOKEN_VAR: &str = "CENTRAL_NATIVE_TOKEN";

const AUTHORITY_PATH: &str = "Control/user/native-action-authority.json";

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

// ---------------------------------------------------------------------------
// The owner command seam
// ---------------------------------------------------------------------------

/// The owner executables, resolved with the suite's existing override
/// conventions (`CENTRAL_CTRL_BIN`, `FACTORY_BIN`, …) before PATH.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerBins {
    pub ctrl: String,
    pub actuation: String,
    pub factory: String,
}

impl Default for OwnerBins {
    fn default() -> Self {
        Self {
            ctrl: "ctrl".into(),
            actuation: "actuation".into(),
            factory: "factory".into(),
        }
    }
}

impl OwnerBins {
    pub fn from_env() -> Self {
        let pick = |keys: &[&str], fallback: &str| {
            keys.iter()
                .find_map(|key| env_nonempty(key))
                .unwrap_or_else(|| fallback.to_owned())
        };
        Self {
            ctrl: pick(&["CENTRAL_CTRL_BIN", "OI_CENTRAL_CTRL_BIN"], "ctrl"),
            actuation: pick(&["ACTUATION_BIN", "OI_ACTUATION_BIN"], "actuation"),
            factory: pick(&["FACTORY_BIN", "OI_FACTORY_BIN"], "factory"),
        }
    }
}

/// What one owner command answered.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Ok(Value),
    /// The verb exists and refused; `code` is the owner's own code.
    Refused {
        command: String,
        code: String,
        message: String,
    },
    /// The owner could not answer: missing binary/verb, timeout, bad output.
    /// `outcome` is a word from the shared probe vocabulary.
    Unavailable {
        command: String,
        outcome: &'static str,
        error: String,
    },
    /// The join's overall budget ended before this command could run.
    Skipped {
        command: String,
        reason: String,
    },
}

impl Answer {
    pub fn ok(&self) -> Option<&Value> {
        match self {
            Self::Ok(value) => Some(value),
            _ => None,
        }
    }

    pub fn refusal_code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } => Some(code),
            _ => None,
        }
    }

    /// The exact failing command and its error, for a facet reason.
    pub fn describe(&self) -> String {
        match self {
            Self::Ok(_) => "answered".into(),
            Self::Refused {
                command,
                code,
                message,
            } => format!("`{command}` refused ({code}): {message}"),
            Self::Unavailable {
                command,
                outcome,
                error,
            } => format!("`{command}` {outcome}: {error}"),
            Self::Skipped { command, reason } => format!("`{command}` not run: {reason}"),
        }
    }

    /// A facet for an answer that did not succeed.
    pub fn failed_facet(&self, source: &str) -> Facet {
        match self {
            Self::Skipped { .. } => Facet::not_attempted(source, self.describe()),
            _ => Facet::unavailable(source, self.describe()),
        }
    }
}

/// Render an argv the way a human would paste it: JSON and spaced arguments
/// single-quoted.
pub fn display_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            if arg.is_empty()
                || arg
                    .chars()
                    .any(|c| c.is_whitespace() || "{}\"'$`[]*?;&|<>()".contains(c))
            {
                format!("'{}'", arg.replace('\'', r"'\''"))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn verb_missing(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    (lower.contains("unknown") || lower.contains("unrecognized"))
        && ["action", "command", "operation", "subcommand"]
            .iter()
            .any(|word| lower.contains(word))
}

fn output_text(output: &Output) -> String {
    let text = if output.stderr.trim().is_empty() {
        output.stdout.trim()
    } else {
        output.stderr.trim()
    };
    let text = text.replace('\n', " ");
    aikit_core::inhabitation::bounded_text(&text, 600)
}

/// Bounded owner calls through one [`CommandRunner`].
pub struct Owners<'a> {
    runner: &'a (dyn CommandRunner + Sync),
    pub bins: OwnerBins,
    pub central_root: Option<PathBuf>,
    per_call: Duration,
    deadline: Instant,
    calls: Mutex<Vec<OwnerCallRecord>>,
}

impl<'a> Owners<'a> {
    pub fn new(
        runner: &'a (dyn CommandRunner + Sync),
        bins: OwnerBins,
        central_root: Option<PathBuf>,
        per_call: Duration,
        total: Duration,
    ) -> Self {
        Self {
            runner,
            bins,
            central_root,
            per_call,
            deadline: Instant::now() + total,
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<OwnerCallRecord> {
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn record(&self, answer: &Answer, command: &str, elapsed: Duration) {
        let (outcome, detail) = match answer {
            Answer::Ok(_) => ("ok".to_owned(), None),
            Answer::Refused { code, .. } => ("refused".to_owned(), Some(code.clone())),
            Answer::Unavailable { outcome, error, .. } => {
                ((*outcome).to_owned(), Some(bounded(error, 240)))
            }
            Answer::Skipped { reason, .. } => ("skipped".to_owned(), Some(reason.clone())),
        };
        self.calls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(OwnerCallRecord {
                command: command.to_owned(),
                outcome,
                elapsed_ms: elapsed.as_millis().min(u64::MAX as u128) as u64,
                detail,
            });
    }

    fn spawn(&self, argv: &[String]) -> std::result::Result<Output, Answer> {
        let command = display_argv(argv);
        let now = Instant::now();
        if now >= self.deadline {
            return Err(Answer::Skipped {
                command,
                reason: "the inhabitation read budget ended before this owner call".into(),
            });
        }
        let budget = self.per_call.min(self.deadline - now);
        match self.runner.run_with_timeout(argv, budget) {
            Ok(output) => Ok(output),
            Err(error) => {
                let outcome = if error.code() == "mux.command_timeout" {
                    "timed-out"
                } else {
                    "unreachable"
                };
                Err(Answer::Unavailable {
                    command,
                    outcome,
                    error: error.message().to_owned(),
                })
            }
        }
    }

    /// `ctrl --json [--root R] action run <action> <input>`.
    pub fn ctrl(&self, action: &str, input: Value) -> Answer {
        let mut argv = vec![self.bins.ctrl.clone(), "--json".into()];
        if let Some(root) = &self.central_root {
            argv.push("--root".into());
            argv.push(root.display().to_string());
        }
        argv.extend(["action", "run", action].map(String::from));
        argv.push(input.to_string());
        let command = display_argv(&argv);
        let started = Instant::now();
        let answer = match self.spawn(&argv) {
            Err(answer) => answer,
            Ok(output) => classify_ctrl(&command, &output),
        };
        self.record(&answer, &command, started.elapsed());
        answer
    }

    /// `actuation occupancy <args…> --json`.
    pub fn occupancy(&self, args: &[&str]) -> Answer {
        let mut argv = vec![self.bins.actuation.clone(), "occupancy".into()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        argv.push("--json".into());
        self.tool(argv)
    }

    /// `factory <args…> --json`.
    pub fn factory(&self, args: &[&str]) -> Answer {
        let mut argv = vec![self.bins.factory.clone()];
        argv.extend(args.iter().map(|arg| (*arg).to_owned()));
        argv.push("--json".into());
        self.tool(argv)
    }

    fn tool(&self, argv: Vec<String>) -> Answer {
        let command = display_argv(&argv);
        let started = Instant::now();
        let answer = match self.spawn(&argv) {
            Err(answer) => answer,
            Ok(output) => classify_tool(&command, &output),
        };
        self.record(&answer, &command, started.elapsed());
        answer
    }
}

fn bounded(text: &str, max: usize) -> String {
    aikit_core::inhabitation::bounded_text(text, max)
}

fn classify_ctrl(command: &str, output: &Output) -> Answer {
    match serde_json::from_str::<Value>(output.stdout.trim()) {
        Ok(envelope) if envelope["ok"] == true => Answer::Ok(envelope["data"].clone()),
        Ok(envelope) if envelope["ok"] == false => {
            let code = envelope["error"]["code"]
                .as_str()
                .unwrap_or("central.refused")
                .to_owned();
            let message = envelope["error"]["message"]
                .as_str()
                .unwrap_or("Central refused without a message")
                .to_owned();
            if verb_missing(&message) {
                Answer::Unavailable {
                    command: command.to_owned(),
                    outcome: "unsupported",
                    error: message,
                }
            } else {
                Answer::Refused {
                    command: command.to_owned(),
                    code,
                    message,
                }
            }
        }
        _ => {
            let text = output_text(output);
            Answer::Unavailable {
                command: command.to_owned(),
                outcome: if verb_missing(&text) {
                    "unsupported"
                } else {
                    "refused"
                },
                error: if output.ok() {
                    format!("owner answered without a JSON envelope: {text}")
                } else {
                    format!("exit {}: {text}", output.status)
                },
            }
        }
    }
}

fn classify_tool(command: &str, output: &Output) -> Answer {
    if output.ok() {
        return match serde_json::from_str::<Value>(output.stdout.trim()) {
            Ok(value) => Answer::Ok(value),
            Err(error) => Answer::Unavailable {
                command: command.to_owned(),
                outcome: "refused",
                error: format!("owner answered without JSON ({error})"),
            },
        };
    }
    let text = output_text(output);
    // A refusal document first — `{ok:false, error:{code, fact, consequence,
    // action}}` (the contract's three-part refusal) on stdout or stderr — so a
    // refusal whose words mention "unknown" is never mistaken for a missing verb.
    let document = [output.stdout.trim(), output.stderr.trim()]
        .into_iter()
        .find_map(|candidate| serde_json::from_str::<Value>(candidate).ok())
        .filter(Value::is_object);
    if document.is_none() && verb_missing(&text) {
        return Answer::Unavailable {
            command: command.to_owned(),
            outcome: "unsupported",
            error: text,
        };
    }
    if let Some(document) = document {
        let error = if document["error"].is_object() {
            &document["error"]
        } else {
            &document
        };
        let code = error["code"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("exit-{}", output.status));
        let message = ["fact", "consequence", "action", "message"]
            .iter()
            .filter_map(|key| error[*key].as_str())
            .collect::<Vec<_>>()
            .join(" · ");
        return Answer::Refused {
            command: command.to_owned(),
            code,
            message: if message.is_empty() { text } else { message },
        };
    }
    Answer::Refused {
        command: command.to_owned(),
        code: format!("exit-{}", output.status),
        message: text,
    }
}

// ---------------------------------------------------------------------------
// Value helpers
// ---------------------------------------------------------------------------

/// The first string field among `keys` (snake_case and camelCase spellings of
/// one owner field are both accepted; nothing else is inferred).
pub fn pick(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    })
}

fn pick_revision(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match value.get(*key) {
        Some(Value::String(text)) if !text.trim().is_empty() => Some(text.clone()),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(Value::Object(object)) => object.get("revision").and_then(|revision| match revision {
            Value::String(text) => Some(text.clone()),
            Value::Number(number) => Some(number.to_string()),
            _ => None,
        }),
        _ => None,
    })
}

fn array<'v>(value: &'v Value, keys: &[&str]) -> Vec<&'v Value> {
    if let Some(items) = value.as_array() {
        return items.iter().collect();
    }
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_array))
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn strings_in(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// The object in `value` (at any depth) whose `position_ref` is `position`.
fn find_position_entry<'v>(value: &'v Value, position: &str) -> Option<&'v Value> {
    match value {
        Value::Object(object) => {
            if pick(value, &["position_ref", "positionRef"]).as_deref() == Some(position) {
                return Some(value);
            }
            object
                .values()
                .find_map(|child| find_position_entry(child, position))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| find_position_entry(child, position)),
        _ => None,
    }
}

/// The refs of a single current work node, from Factory's
/// `factory.current-work/v1` answer: the node itself (`current.node_ref`, its
/// singleton `work_refs` / `journey_refs` / `custody_refs`) joined with the
/// resolved candidates naming that node (which carry the Run, WorkflowUnit and
/// status). Direct keys on `current` are honoured too.
pub fn current_work_refs(reading: &Value) -> std::collections::BTreeMap<String, String> {
    let mut refs = std::collections::BTreeMap::new();
    let current = &reading["current"];
    let node = pick(current, &["node_ref", "nodeRef"]);
    for key in [
        "custody_ref",
        "work_ref",
        "run_ref",
        "journey_ref",
        "workflow_unit_ref",
        "state",
    ] {
        if let Some(value) = pick(current, &[key, camel(key).as_str()]) {
            refs.insert(key.to_owned(), value);
        }
    }
    for (plural, key) in [
        ("work_refs", "work_ref"),
        ("journey_refs", "journey_ref"),
        ("custody_refs", "custody_ref"),
    ] {
        let values = array(current, &[plural, camel(plural).as_str()]);
        if let [only] = values.as_slice() {
            if let Some(value) = only.as_str() {
                refs.entry(key.to_owned())
                    .or_insert_with(|| value.to_owned());
            }
        }
    }
    for candidate in array(reading, &["candidates"]) {
        let resolved = pick(candidate, &["resolution"]).is_none_or(|r| r == "resolved");
        let same_node = match (&node, pick(candidate, &["node_ref", "nodeRef"])) {
            (Some(node), Some(candidate_node)) => *node == candidate_node,
            _ => true,
        };
        if !resolved || !same_node {
            continue;
        }
        for key in ["work_ref", "run_ref", "journey_ref", "workflow_unit_ref"] {
            if let Some(value) = pick(candidate, &[key, camel(key).as_str()]) {
                refs.entry(key.to_owned()).or_insert(value);
            }
        }
        if let Some(status) = pick(candidate, &["status"]) {
            refs.entry("state".to_owned()).or_insert(status);
        }
        if pick(candidate, &["source"]).as_deref() == Some("custody") {
            if let Some(custody) = pick(candidate, &["source_ref", "sourceRef"]) {
                refs.entry("custody_ref".to_owned()).or_insert(custody);
            }
        }
    }
    if let Some(node) = node {
        refs.entry("work_ref".to_owned()).or_insert(node);
    }
    refs
}

/// A digest of the current-work answer that changes exactly when the work
/// the Position carries changes (outcome and the refs it names).
pub fn current_work_digest(reading: &Value) -> Option<String> {
    let outcome = pick(reading, &["outcome"])?;
    let refs = |value: &Value| {
        [
            "custody_ref",
            "work_ref",
            "run_ref",
            "journey_ref",
            "workflow_unit_ref",
            "state",
        ]
        .iter()
        .map(|key| {
            let camel = camel(key);
            (
                (*key).to_owned(),
                pick(value, &[key, camel.as_str()]).unwrap_or_default(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>()
    };
    let mut candidates: Vec<_> = array(reading, &["candidates"])
        .into_iter()
        .map(refs)
        .collect();
    candidates.sort();
    let basis = json!({
        "outcome": outcome,
        "current": reading.get("current").map(refs),
        "candidates": candidates,
    });
    Some(format!(
        "blake3:{}",
        blake3::hash(basis.to_string().as_bytes()).to_hex()
    ))
}

fn camel(snake: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for ch in snake.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Central's path escape for `central.path-ref/v1` refs.
fn central_path_escape(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"/-_.".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn central_path_ref(root: &str, path: &str) -> Value {
    json!({
        "schema": "central.path-ref/v1",
        "ref": format!("central:path:{}:{}", central_path_escape(root), central_path_escape(path)),
        "root": root,
        "path": path,
    })
}

/// AIKit's own filesystem recognition of `<root>/Work/<name>` — used only to
/// address downstream Central inputs when `central.world.here` cannot answer,
/// and always disclosed as such.
fn recognised_project(central_root: Option<&Path>, cwd: &Path) -> Option<(String, PathBuf)> {
    let root = central_root?;
    let name = crate::orientation_packet::project_of(root, cwd)?;
    Some((name.clone(), root.join("Work").join(name)))
}

// ---------------------------------------------------------------------------
// The join
// ---------------------------------------------------------------------------

/// What the caller knows before any owner is asked.
#[derive(Debug, Clone)]
pub struct JoinInput {
    pub cwd: PathBuf,
    pub position_flag: Option<String>,
    pub env_position: Option<String>,
    pub env_generation: Option<String>,
    /// Current AgentSession identifiers, each with where it came from
    /// (`--agent-session`, `AIKIT_SESSION_ID`, hook `session_id`).
    pub agent_sessions: Vec<(String, String)>,
    pub depth: ReadingDepth,
    /// Whether step 3 of the resolution chain may ask Actuation for the whole
    /// occupancy list. Hooks set this only when an occupancy store exists.
    pub allow_session_lookup: bool,
}

impl JoinInput {
    pub fn from_process(cwd: PathBuf, depth: ReadingDepth) -> Self {
        let mut agent_sessions = Vec::new();
        if let Some(session) = env_nonempty(AGENT_SESSION_VAR) {
            agent_sessions.push((AGENT_SESSION_VAR.to_owned(), session));
        }
        Self {
            cwd,
            position_flag: None,
            env_position: env_nonempty(POSITION_VAR),
            env_generation: env_nonempty(GENERATION_VAR),
            agent_sessions,
            depth,
            allow_session_lookup: true,
        }
    }
}

/// Reads AIKit itself answers (SessionSpace, prepared context, composition).
#[derive(Default)]
pub struct AikitReads<'a> {
    pub home: Option<&'a AikitHome>,
    /// participant ref → prepared NOW meta (`Ok(None)` = nothing prepared).
    #[allow(clippy::type_complexity)]
    pub prepared: Option<&'a dyn Fn(&str) -> Result<Option<Value>>>,
    /// Why no prepared-context reader was supplied, when none was.
    pub prepared_absent_reason: Option<String>,
    /// The ActorBootstrap composition (`aikit compose`), joined at full depth.
    pub compose: Option<&'a dyn Fn() -> Result<Value>>,
}

/// Intermediate owner answers a later facet (or Refocus) reuses.
#[derive(Debug, Clone, Default)]
pub struct JoinTrail {
    pub central_root: Option<String>,
    pub project_name: Option<String>,
    pub project_root: Option<PathBuf>,
    pub workcell_ref: Option<String>,
    /// The current Workcell as `central.world.here` answered it (carries its
    /// read-only `root_now` state).
    pub workcell: Option<Value>,
    pub tenure: Option<Value>,
    pub occupancy_list: Option<Value>,
    pub factory_state: Option<String>,
    pub current_work: Option<Value>,
    pub workflow_unit: Option<Value>,
    pub position_record: Option<Value>,
    pub peers: Vec<Value>,
}

pub struct Joined {
    pub reading: InhabitationReading,
    pub trail: JoinTrail,
}

fn position_list_hint(project: Option<&str>) -> String {
    match project {
        Some(project) => {
            format!("ctrl --json action run central.position.list '{{\"project\":\"{project}\"}}'")
        }
        None => "ctrl --json action run central.position.list '{}'".to_owned(),
    }
}

/// Join every owner into one reading. Never fails: every failure is a facet.
pub fn join(owners: &Owners<'_>, input: &JoinInput, reads: &AikitReads<'_>) -> Joined {
    let mut facets =
        InhabitationFacets::unattempted("aikit whoami", "not reached by this reading's depth");
    let mut identity = InhabitationIdentity::default();
    let mut trail = JoinTrail::default();
    let standard = input.depth != ReadingDepth::Lean;

    // -- Local World, Project World, Workcell ------------------------------
    let here_source = "central.world.here";
    let here = owners.ctrl(
        "central.world.here",
        json!({ "cwd": input.cwd.display().to_string() }),
    );
    match here.ok() {
        Some(data) => {
            let local = &data["local_world"];
            if local.is_object() {
                let reference = pick(local, &["ref"]).unwrap_or_else(|| "control:root".into());
                let root = pick(local, &["root"]);
                trail.central_root = root.clone();
                identity.local_world_ref = Some(reference.clone());
                facets.local_world = Facet::present(
                    here_source,
                    match &root {
                        Some(root) => format!("{reference} ({root})"),
                        None => reference.clone(),
                    },
                    local.clone(),
                );
            } else {
                facets.local_world =
                    Facet::absent(here_source, "Central answered without a local_world");
            }
            let project = &data["project_world"];
            let state = pick(project, &["state"]).unwrap_or_else(|| "absent".into());
            match state.as_str() {
                "present" => {
                    let reference = pick(project, &["ref"]).unwrap_or_default();
                    let path = pick(project, &["path"]);
                    trail.project_name = pick(project, &["name"]);
                    trail.project_root = match (&trail.central_root, &path) {
                        (Some(root), Some(path)) => Some(Path::new(root).join(path)),
                        _ => None,
                    };
                    identity.project_world_ref = Some(reference.clone());
                    facets.project_world = Facet::present(
                        here_source,
                        format!(
                            "{reference} ({}, via {})",
                            path.as_deref().unwrap_or("?"),
                            pick(project, &["via"]).unwrap_or_else(|| "cwd".into())
                        ),
                        project.clone(),
                    );
                }
                "ambiguous" => {
                    facets.project_world = Facet::ambiguous(
                        here_source,
                        "more than one Work member claims this path",
                        project.clone(),
                    )
                    .with_next("aikit whoami -C <project root>");
                }
                _ => {
                    let relation =
                        pick(&data["cwd"], &["relation"]).unwrap_or_else(|| "outside".into());
                    facets.project_world = Facet::absent(
                        here_source,
                        format!(
                            "{} is {relation} — no Project World encloses it",
                            input.cwd.display()
                        ),
                    );
                }
            }
            let current = array(&data["workcells"], &[])
                .into_iter()
                .find(|cell| pick(cell, &["role"]).as_deref() == Some("current"))
                .cloned();
            match current {
                Some(cell) => {
                    let reference = pick(&cell, &["ref"]).unwrap_or_default();
                    trail.workcell_ref = Some(reference.clone());
                    trail.workcell = Some(cell.clone());
                    identity.workcell_ref = Some(reference.clone());
                    facets.workcell = Facet::present(
                        here_source,
                        format!(
                            "{reference} (declared by {})",
                            pick(&cell, &["declared_by"]).unwrap_or_else(|| "?".into())
                        ),
                        cell,
                    );
                }
                None => {
                    facets.workcell = Facet::absent(
                        here_source,
                        "no current Workcell is declared (Control/machines/current.json)",
                    )
                }
            }
        }
        None => {
            let reason = here.describe();
            facets.local_world = here.failed_facet(here_source);
            facets.project_world = here.failed_facet(here_source);
            facets.workcell = here.failed_facet(here_source);
            // Downstream Central inputs still need a project name; AIKit's
            // own recognition of the Work tree supplies it, disclosed.
            if let Some((name, root)) =
                recognised_project(owners.central_root.as_deref(), &input.cwd)
            {
                facets.project_world = facets.project_world.because(format!(
                    "{reason}; AIKit recognises {} as Work member `{name}` (filesystem only, unconfirmed by Central)",
                    input.cwd.display()
                ));
                trail.project_name = Some(name);
                trail.project_root = Some(root);
            }
            trail.central_root = owners
                .central_root
                .as_ref()
                .map(|root| root.display().to_string());
        }
    }

    // -- Position resolution: flag → env → AgentSession occupancy → absent --
    let mut resolved_by = ResolvedBy::None;
    let mut position: Option<String> = None;
    let mut generation: Option<String> = None;
    let mut session_match_note: Option<String> = None;
    if let Some(flag) = &input.position_flag {
        resolved_by = ResolvedBy::Flag;
        let flag = match flag.strip_prefix('@') {
            // An `@handle` names one Position in this Project World (own or
            // inherited) or none; two is a refusal, never a pick.
            Some(handle) => match handle_to_ref(owners, handle, trail.project_name.as_deref()) {
                Ok(reference) => {
                    session_match_note = Some(format!("@{handle} names {reference}"));
                    Some(reference)
                }
                Err(facet) => {
                    facets.position = *facet;
                    None
                }
            },
            None => Some(flag.clone()),
        };
        if let Some(flag) = flag {
            if input.env_position.as_deref() == Some(flag.as_str()) {
                generation = input.env_generation.clone();
            }
            position = Some(flag);
        }
    } else if let Some(env) = &input.env_position {
        resolved_by = ResolvedBy::Env;
        position = Some(env.clone());
        generation = input.env_generation.clone();
    }
    let mut occupancy_list: Option<Answer> = None;
    if position.is_none() && resolved_by != ResolvedBy::Flag {
        if input.agent_sessions.is_empty() {
            session_match_note = Some(
                "no --position, no OI_POSITION_REF, and no current AgentSession identifier (--agent-session or AIKIT_SESSION_ID) to match an occupancy".into(),
            );
        } else if !input.allow_session_lookup {
            session_match_note = Some(
                "no --position or OI_POSITION_REF, and no Actuation occupancy store exists to match this AgentSession against".into(),
            );
        } else {
            let list = owners.occupancy(&["list"]);
            match list.ok() {
                Some(data) => {
                    let matches: Vec<(String, Value, String)> =
                        array(data, &["positions", "occupancies", "entries"])
                            .into_iter()
                            .filter_map(|entry| {
                                let current = entry.get("current")?;
                                let session =
                                    pick(current, &["agent_session_ref", "agentSessionRef"])?;
                                let (label, _) =
                                    input.agent_sessions.iter().find(|(_, id)| id == &session)?;
                                Some((
                                    pick(entry, &["position_ref", "positionRef"])?,
                                    current.clone(),
                                    label.clone(),
                                ))
                            })
                            .collect();
                    match matches.len() {
                        1 => {
                            let (matched, current, label) =
                                matches.into_iter().next().unwrap_or_default();
                            resolved_by = ResolvedBy::AgentSession;
                            generation = pick(&current, &["generation_ref", "generationRef"]);
                            session_match_note = Some(format!(
                                "the open tenure of {matched} names this AgentSession ({label})"
                            ));
                            position = Some(matched);
                        }
                        0 => {
                            session_match_note = Some(format!(
                                "no open tenure names this AgentSession ({})",
                                input
                                    .agent_sessions
                                    .iter()
                                    .map(|(label, id)| format!("{label}={id}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ))
                        }
                        _ => {
                            let candidates: Vec<String> = matches
                                .iter()
                                .map(|(position, _, _)| position.clone())
                                .collect();
                            facets.position = Facet::ambiguous(
                                "actuation occupancy list",
                                format!(
                                    "{} open tenures name this AgentSession — refusing to pick one",
                                    candidates.len()
                                ),
                                json!({ "candidates": candidates }),
                            )
                            .with_next("aikit whoami --position <one of the candidates>");
                        }
                    }
                }
                None => {
                    session_match_note = Some(list.describe());
                    facets.position = list.failed_facet("actuation occupancy list");
                }
            }
            if let Answer::Ok(data) = &list {
                trail.occupancy_list = Some(data.clone());
            }
            occupancy_list = Some(list);
        }
    }

    // -- Position definition -------------------------------------------------
    let position_source = "central.position.read";
    if let Some(position_ref) = &position {
        identity.position_ref = Some(position_ref.clone());
        let read = owners.ctrl(
            "central.position.read",
            json!({ "position_ref": position_ref }),
        );
        let via = format!(
            "resolved by {}{}",
            resolved_by.as_str(),
            session_match_note
                .as_deref()
                .map(|note| format!(": {note}"))
                .unwrap_or_default()
        );
        match &read {
            Answer::Ok(data) => {
                let record = data.get("record").unwrap_or(data);
                let revision = pick_revision(record, &["revision"])
                    .or_else(|| pick_revision(&data["source"], &["revision"]));
                identity.position_revision = revision.clone();
                trail.position_record = Some(record.clone());
                let label = pick(record, &["label"]);
                let handle = pick(record, &["handle"]);
                facets.position = Facet::present(
                    position_source,
                    format!(
                        "{position_ref}{}{}{}",
                        label.map(|l| format!(" \"{l}\"")).unwrap_or_default(),
                        handle.map(|h| format!(" ({h})")).unwrap_or_default(),
                        revision.map(|r| format!(" @{r}")).unwrap_or_default()
                    ),
                    data.clone(),
                )
                .because(via);
            }
            Answer::Refused { code, .. } if code.contains("not_found") => {
                facets.position =
                    Facet::absent(position_source, format!("{} ({via})", read.describe()))
                        .with_summary(position_ref.clone())
                        .with_next(position_list_hint(trail.project_name.as_deref()));
            }
            failure => {
                facets.position = failure
                    .failed_facet(position_source)
                    .with_summary(position_ref.clone())
                    .with_value(json!({ "position_ref": position_ref }))
                    .because(format!("{} ({via})", failure.describe()));
            }
        }
    } else if facets.position.state == FacetState::NotAttempted {
        facets.position = Facet::absent(
            "aikit whoami resolution chain",
            session_match_note.unwrap_or_else(|| "no Position resolved".into()),
        )
        .with_next(format!(
            "aikit whoami --position <ref>  (Positions: {})",
            position_list_hint(trail.project_name.as_deref())
        ));
    }

    // -- Occupancy and the tenure-derived facets -----------------------------
    let occupancy_source = "actuation occupancy";
    let mut holds_position = false;
    if let Some(position_ref) = &position {
        let mut verified: Option<bool> = None;
        let mut verify_note: Option<String> = None;
        if resolved_by != ResolvedBy::AgentSession {
            if let Some(generation_ref) = &generation {
                let verify = owners.occupancy(&[
                    "verify",
                    "--position",
                    position_ref,
                    "--generation",
                    generation_ref,
                ]);
                match &verify {
                    Answer::Ok(_) => verified = Some(true),
                    Answer::Refused { .. } => {
                        verified = Some(false);
                        verify_note = Some(verify.describe());
                    }
                    other => verify_note = Some(other.describe()),
                }
            }
        } else {
            verified = Some(true);
        }
        let read = owners.occupancy(&["read", "--position", position_ref]);
        match read.ok() {
            Some(data) => {
                let occupied = pick(data, &["state"]).as_deref() == Some("occupied");
                let current = data.get("current").filter(|c| c.is_object()).cloned();
                let current_generation = current
                    .as_ref()
                    .and_then(|c| pick(c, &["generation_ref", "generationRef"]));
                if verified == Some(false) {
                    facets.occupancy = Facet::absent(
                        occupancy_source,
                        format!(
                            "this body's generation {} is not the current occupant of {position_ref}: {}",
                            generation.as_deref().unwrap_or("?"),
                            verify_note.clone().unwrap_or_default()
                        ),
                    )
                    .with_value(data.clone())
                    .with_next(format!(
                        "actuation occupancy read --position {position_ref} --json"
                    ));
                } else if !occupied {
                    facets.occupancy = Facet::absent(
                        occupancy_source,
                        format!("{position_ref} is vacant"),
                    )
                    .with_value(data.clone())
                    .with_next(format!(
                        "actuation occupancy claim --position {position_ref} --agent <agent> --agency <agency> --reason <why> --expect-vacant --json"
                    ));
                } else if generation.is_some() && current_generation != generation {
                    facets.occupancy = Facet::absent(
                        occupancy_source,
                        format!(
                            "this body's generation {} is not the open tenure ({})",
                            generation.as_deref().unwrap_or("?"),
                            current_generation.as_deref().unwrap_or("?")
                        ),
                    )
                    .with_value(data.clone());
                } else {
                    let tenure = current.clone().unwrap_or(Value::Null);
                    holds_position = generation.is_some();
                    let ordinal = tenure
                        .get("generation_ordinal")
                        .or_else(|| tenure.get("generationOrdinal"))
                        .and_then(Value::as_u64);
                    identity.occupant_generation = current_generation.clone();
                    identity.generation_ordinal = ordinal;
                    let proof = match (verified, &verify_note, generation.is_some()) {
                        (Some(true), _, _) => "verified by actuation occupancy verify".to_owned(),
                        (None, Some(note), _) => format!("unverified: {note}"),
                        (_, _, false) => "the current occupant; this body carries no occupant generation, so it is not proven to hold the Position".to_owned(),
                        _ => "unverified".to_owned(),
                    };
                    facets.occupancy = Facet::present(
                        occupancy_source,
                        format!(
                            "occupied by generation {}{} ({})",
                            ordinal.map(|o| format!("#{o} ")).unwrap_or_default(),
                            current_generation.as_deref().unwrap_or("?"),
                            proof
                        ),
                        data.clone(),
                    );
                    trail.tenure = Some(tenure);
                }
            }
            None => {
                facets.occupancy = read.failed_facet(occupancy_source).because(format!(
                    "{}{}",
                    read.describe(),
                    verify_note
                        .as_deref()
                        .map(|note| format!("; verify: {note}"))
                        .unwrap_or_default()
                ));
                if let Some(generation_ref) = &generation {
                    // The launch stamp is testimony, not proof; carry it as
                    // the facet's value without claiming occupancy.
                    facets.occupancy = facets
                        .occupancy
                        .with_value(json!({ "stamped_generation": generation_ref }));
                }
            }
        }
    } else {
        facets.occupancy = Facet::not_attempted(
            occupancy_source,
            "no Position resolved to read occupancy for",
        );
    }
    let tenure = trail.tenure.clone();
    let tenure_source = "actuation occupancy read (open tenure)";
    let tenure_field = |keys: &[&str]| tenure.as_ref().and_then(|t| pick(t, keys));
    let no_tenure = |facet: &str| {
        if position.is_none() {
            Facet::not_attempted(
                tenure_source,
                format!("no Position resolved; {facet} comes from the open tenure"),
            )
        } else {
            Facet::absent(
                tenure_source,
                format!("no open tenure is readable for this Position, so no {facet} is named"),
            )
        }
    };
    let occupant_note = if holds_position {
        None
    } else {
        Some("the Position's current occupant — not proven to be this body".to_owned())
    };
    for (name, keys) in [
        ("agent", &["agent_ref", "agentRef"][..]),
        ("agency", &["agency_ref", "agencyRef"][..]),
    ] {
        let facet = match (&tenure, tenure_field(keys)) {
            (Some(_), Some(reference)) => {
                if name == "agent" {
                    identity.agent_ref = Some(reference.clone());
                } else {
                    identity.agency_ref = Some(reference.clone());
                }
                Facet::present(
                    tenure_source,
                    reference.clone(),
                    json!({ "ref": reference }),
                )
                .because(occupant_note.clone().unwrap_or_default())
            }
            (Some(_), None) => {
                Facet::absent(tenure_source, format!("the open tenure names no {name}"))
            }
            (None, _) => no_tenure(name),
        };
        if let Some(target) = facets.get_mut(name) {
            *target = facet;
        }
    }
    let current_sessions: Vec<Value> = input
        .agent_sessions
        .iter()
        .map(|(label, id)| json!({ "source": label, "id": id }))
        .collect();
    facets.agent_session = match (
        &tenure,
        tenure_field(&["agent_session_ref", "agentSessionRef"]),
    ) {
        (Some(_), Some(reference)) => {
            identity.agent_session_ref = Some(reference.clone());
            let matches = input.agent_sessions.iter().any(|(_, id)| id == &reference);
            Facet::present(
                tenure_source,
                reference.clone(),
                json!({ "ref": reference, "current_identifiers": current_sessions, "matches_current": matches }),
            )
            .because(if matches || input.agent_sessions.is_empty() {
                String::new()
            } else {
                "the tenure names a different AgentSession than this process carries".into()
            })
        }
        (Some(_), None) => Facet::absent(tenure_source, "the open tenure names no AgentSession")
            .with_value(json!({ "current_identifiers": current_sessions })),
        (None, _) => {
            let mut facet = no_tenure("AgentSession");
            if !current_sessions.is_empty() {
                facet = facet.with_value(json!({ "current_identifiers": current_sessions }));
            }
            facet
        }
    };
    let space_ref = tenure_field(&["session_space_ref", "sessionSpaceRef"]);
    let space_state = match (&space_ref, reads.home) {
        (Some(space), Some(home)) => Some(
            aikit_core::session_space::SessionSpaceRef::parse(space).and_then(|reference| {
                aikit_store::SessionSpaceApplicationStore::new(home.clone()).load(&reference)
            }),
        ),
        _ => None,
    };
    facets.session_space = match (&space_ref, &space_state) {
        (Some(space), Some(Ok(state))) => {
            identity.session_space_ref = Some(space.clone());
            Facet::present(
                "tenure + AIKit SessionSpace store",
                format!("{space} @r{}", state.revision),
                json!({ "ref": space, "revision": state.revision, "label": state.label, "agent_sessions": state.agent_sessions.len(), "working_surfaces": state.working_surfaces.len() }),
            )
        }
        (Some(space), Some(Err(error))) => {
            identity.session_space_ref = Some(space.clone());
            Facet::present(tenure_source, space.clone(), json!({ "ref": space })).because(format!(
                "named by the tenure; this AIKit home cannot load it: {}",
                error.message()
            ))
        }
        (Some(space), None) => {
            identity.session_space_ref = Some(space.clone());
            Facet::present(tenure_source, space.clone(), json!({ "ref": space }))
        }
        (None, _) => {
            if tenure.is_some() {
                Facet::absent(tenure_source, "the open tenure names no SessionSpace")
            } else {
                no_tenure("SessionSpace")
            }
        }
    };
    facets.working_surface = match (&space_state, &identity.agent_session_ref) {
        (Some(Ok(state)), Some(agent_session)) => {
            let bindings: Vec<_> = state
                .working_surfaces
                .values()
                .filter(|binding| binding.agent_session.as_str() == agent_session)
                .collect();
            let space = space_ref.clone().unwrap_or_default();
            match bindings.len() {
                1 => Facet::present(
                    "AIKit SessionSpace working-surface binding",
                    format!(
                        "{} → surface {} via {}",
                        bindings[0].binding, bindings[0].surface, bindings[0].provider
                    ),
                    serde_json::to_value(bindings[0]).unwrap_or(Value::Null),
                )
                .with_next(format!(
                    "aikit session-space working-surface open {space} {}",
                    bindings[0].binding
                )),
                0 => Facet::absent(
                    "AIKit SessionSpace working-surface binding",
                    format!("{space} binds no working Surface for {agent_session}"),
                ),
                n => Facet::ambiguous(
                    "AIKit SessionSpace working-surface binding",
                    format!("{n} working-Surface bindings name {agent_session}"),
                    json!(bindings
                        .iter()
                        .map(|b| b.binding.to_string())
                        .collect::<Vec<_>>()),
                ),
            }
        }
        (Some(Err(_)), _) => Facet::unavailable(
            "AIKit SessionSpace store",
            "the tenure's SessionSpace is not loadable from this AIKit home",
        ),
        _ => Facet::not_attempted(
            "AIKit SessionSpace working-surface binding",
            "needs a SessionSpace and an AgentSession named by the open tenure",
        ),
    };
    let harness = tenure_field(&["harness_composition_ref", "harnessCompositionRef"]);
    let model = tenure_field(&["model_ref", "modelRef"]);
    let mut body_value = json!({ "harness_composition_ref": harness, "model_ref": model });
    let mut body_facet = match (&tenure, &harness, &model) {
        (Some(_), None, None) => Facet::absent(
            tenure_source,
            "the open tenure names no harness composition or model",
        ),
        (Some(_), _, _) => Facet::present(
            tenure_source,
            format!(
                "harness {} · model {}",
                harness.as_deref().unwrap_or("unnamed"),
                model.as_deref().unwrap_or("unnamed")
            ),
            body_value.clone(),
        ),
        (None, _, _) => no_tenure("body"),
    }
    .with_next("aikit compose --json (ActorBootstrap composition)");
    if input.depth == ReadingDepth::Full {
        match reads.compose {
            Some(compose) => match compose() {
                Ok(plan) => {
                    let bootstrap = plan
                        .get("plan")
                        .or_else(|| plan.get("bootstrap"))
                        .unwrap_or(&plan);
                    body_value["actor_bootstrap"] = json!({
                        "harness": bootstrap.get("harness"),
                        "model": bootstrap.get("model"),
                        "agent_session": bootstrap.get("agent_session"),
                        "runtime_body": bootstrap.get("runtime_body"),
                        "harness_candidates": bootstrap.get("harness_candidates").and_then(Value::as_array).map(Vec::len),
                    });
                    let named = |key: &str| {
                        bootstrap
                            .get(key)
                            .and_then(|reference| reference.get("id").or(Some(reference)))
                            .filter(|reference| !reference.is_null())
                            .map(|reference| match reference {
                                Value::String(text) => text.clone(),
                                other => other.to_string(),
                            })
                    };
                    let (harness, model) = (named("harness"), named("model"));
                    body_facet = if body_facet.is_present() {
                        body_facet.with_value(body_value.clone())
                    } else if harness.is_some() || model.is_some() {
                        Facet::present(
                            "aikit compose (ActorBootstrap)",
                            format!(
                                "harness {} · model {} (ActorBootstrap)",
                                harness.as_deref().unwrap_or("unselected"),
                                model.as_deref().unwrap_or("unselected")
                            ),
                            body_value.clone(),
                        )
                    } else {
                        // A composition that selects nothing names no body.
                        let reason = format!(
                            "{}; the ActorBootstrap composition selects no harness or model",
                            body_facet.reason.clone().unwrap_or_default()
                        );
                        body_facet.with_value(body_value.clone()).because(reason)
                    };
                }
                Err(error) => {
                    let reason = format!(
                        "{}; ActorBootstrap composition unavailable: {}",
                        body_facet.reason.clone().unwrap_or_default(),
                        error.message()
                    );
                    body_facet = body_facet.because(reason);
                }
            },
            None => {
                body_facet =
                    body_facet.because("ActorBootstrap composition not joinable from this surface")
            }
        }
    }
    facets.body = body_facet;
    if let (Some(tenure_cell), Some(current)) = (
        tenure_field(&["workcell_ref", "workcellRef"]),
        trail.workcell_ref.clone(),
    ) {
        if tenure_cell != current {
            facets.workcell = facets.workcell.clone().because(format!(
                "this machine is {current}; the occupancy was claimed on {tenure_cell}"
            ));
        }
    }

    // -- Current work (Factory custody) --------------------------------------
    let work_source = "factory development current-work";
    if let (Some(position_ref), Some(project_root)) = (&position, &trail.project_root) {
        let root = project_root.display().to_string();
        let locate = owners.factory(&["project", "locate", &root]);
        match locate
            .ok()
            .and_then(|data| pick(data, &["statePath", "state_path"]))
        {
            Some(state) => {
                trail.factory_state = Some(state.clone());
                let work = owners.factory(&[
                    "development",
                    "current-work",
                    &state,
                    "--position",
                    position_ref,
                ]);
                match work.ok() {
                    Some(data) => {
                        trail.current_work = Some(data.clone());
                        identity.current_work_digest = current_work_digest(data);
                        let outcome = pick(data, &["outcome"]).unwrap_or_else(|| "none".into());
                        identity.current_work_outcome = Some(outcome.clone());
                        let basis = pick(data, &["basis"]).unwrap_or_default();
                        match outcome.as_str() {
                            "one" => {
                                let current = &data["current"];
                                identity.current_work_refs = current_work_refs(data);
                                let refs = &identity.current_work_refs;
                                facets.current_work = Facet::present(
                                    work_source,
                                    format!(
                                        "{} [{}] run {}",
                                        refs.get("work_ref")
                                            .or(refs.get("custody_ref"))
                                            .map(String::as_str)
                                            .unwrap_or("?"),
                                        refs.get("state")
                                            .cloned()
                                            .or_else(|| pick(current, &["state"]))
                                            .unwrap_or_else(|| "in-progress".into()),
                                        refs.get("run_ref").map(String::as_str).unwrap_or("-")
                                    ),
                                    data.clone(),
                                )
                                .because(basis);
                            }
                            "ambiguous" => {
                                facets.current_work = Facet::ambiguous(
                                    work_source,
                                    format!("{basis} — Factory refuses to guess"),
                                    data.clone(),
                                )
                                .with_summary(format!(
                                    "{} candidate work nodes",
                                    array(data, &["candidates"]).len()
                                ));
                            }
                            _ => {
                                facets.current_work = Facet::absent(
                                    work_source,
                                    if basis.is_empty() { "no in-progress custody or active attempt names this Position".into() } else { basis },
                                )
                                .with_value(data.clone());
                            }
                        }
                    }
                    None => facets.current_work = work.failed_facet(work_source),
                }
            }
            None => {
                facets.current_work = if locate.ok().is_some() {
                    Facet::absent(
                        "factory project locate",
                        "Factory located the project but names no developmental state",
                    )
                } else {
                    locate.failed_facet("factory project locate")
                };
            }
        }
    } else {
        facets.current_work = Facet::not_attempted(
            work_source,
            if position.is_none() {
                "no Position resolved; current work is Position custody"
            } else {
                "no Project World root to locate Factory state from"
            },
        );
    }
    // The workflow unit carries the Return address the current work owes.
    if let (Some(state), Some(unit)) = (
        trail.factory_state.clone(),
        identity.current_work_refs.get("workflow_unit_ref").cloned(),
    ) {
        let mut args = vec![
            "development",
            "workflow-unit",
            state.as_str(),
            unit.as_str(),
        ];
        let run = identity.current_work_refs.get("run_ref").cloned();
        if let Some(run) = &run {
            args.push(run.as_str());
        }
        if let Some(data) = owners.factory(&args).ok() {
            trail.workflow_unit = Some(data.clone());
        }
    }

    // -- NOW horizon: Workcell root NOW, then the child NOW -----------------
    // Central reports each Workcell's root NOW read-only in world.here; the
    // ensuring Action (`central.now.workcell-root`) mutates, so a reading
    // only names it as the next step and never runs it.
    let root_source = "central.world.here (workcell root_now)";
    let mut root_now: Option<String> = None;
    match (&trail.workcell_ref, &trail.workcell) {
        (Some(workcell), Some(cell)) => {
            let ensure = format!(
                "ctrl --json action run central.now.workcell-root '{{\"workcell_ref\":\"{workcell}\"}}'"
            );
            match cell.get("root_now").filter(|root| root.is_object()) {
                Some(root) => match pick(root, &["state"]).as_deref() {
                    Some("present") => match pick(root, &["now_ref"]) {
                        Some(now_ref) => {
                            let (facet, revision) = read_now(owners, &now_ref, root_source, "root");
                            identity.root_now_ref = Some(now_ref.clone());
                            identity.root_now_revision = revision;
                            facets.root_now = facet;
                            root_now = Some(now_ref);
                        }
                        None => {
                            facets.root_now = Facet::absent(
                                root_source,
                                "Central reported a present root NOW without its now_ref",
                            )
                        }
                    },
                    Some("absent") => {
                        facets.root_now = Facet::absent(
                            root_source,
                            pick(root, &["reason"]).unwrap_or_else(|| {
                                format!("no root NOW is allocated for {workcell}")
                            }),
                        )
                        .with_value(root.clone())
                        .with_next(ensure)
                    }
                    _ => {
                        facets.root_now = Facet::unavailable(
                            root_source,
                            pick(root, &["reason"]).unwrap_or_else(|| {
                                "Central could not read the Workcell root NOW".into()
                            }),
                        )
                    }
                },
                None => {
                    facets.root_now = Facet::absent(
                        root_source,
                        format!("Central's world.here names no root NOW for {workcell}"),
                    )
                    .with_next(ensure)
                }
            }
        }
        _ => {
            facets.root_now = Facet::not_attempted(
                root_source,
                "no current Workcell is known, so its root NOW cannot be addressed",
            )
        }
    }
    let child_source = "child NOW";
    if let Some(position_ref) = &position {
        let mut candidates: Vec<(String, &'static str)> = Vec::new();
        let mut notes: Vec<String> = Vec::new();
        if let (Some(state), Some(run)) = (
            &trail.factory_state,
            identity.current_work_refs.get("run_ref"),
        ) {
            let inhabitation = owners.factory(&[
                "development",
                "inhabitation",
                state,
                "--run",
                run,
                "--position",
                position_ref,
            ]);
            match inhabitation.ok() {
                Some(data) => {
                    let entry = find_position_entry(data, position_ref);
                    let placement = entry.and_then(|entry| {
                        pick(
                            entry,
                            &["placement_now_ref", "placementNowRef", "now_ref", "nowRef"],
                        )
                        .or_else(|| {
                            entry
                                .get("placement")
                                .and_then(|p| pick(p, &["now_ref", "nowRef"]))
                        })
                    });
                    match placement {
                        Some(now_ref) => candidates
                            .push((now_ref, "factory development inhabitation (placement NOW)")),
                        None => {
                            notes.push("Factory holds no placement NOW for this Position".into())
                        }
                    }
                }
                None => notes.push(inhabitation.describe()),
            }
        }
        // Rows `central.now.children` already answered with their revision.
        let mut rows: Vec<Value> = Vec::new();
        if candidates.is_empty() {
            if let Some(root) = &root_now {
                let children = owners.ctrl("central.now.children", json!({ "now_ref": root }));
                match children.ok() {
                    Some(data) => {
                        for child in array(data, &["children", "records"]) {
                            let participants = strings_in(child, "participant_refs");
                            let names_position = participants.iter().any(|p| p == position_ref)
                                || generation
                                    .as_ref()
                                    .is_some_and(|g| participants.contains(g))
                                || pick(child, &["position_ref"]).as_deref()
                                    == Some(position_ref.as_str());
                            if names_position {
                                if let Some(now_ref) = pick(child, &["now_ref"]) {
                                    candidates.push((now_ref, "central.now.children"));
                                    rows.push(child.clone());
                                }
                            }
                        }
                        if candidates.is_empty() {
                            notes.push(format!("no child of {root} names {position_ref}"));
                        }
                    }
                    None => notes.push(children.describe()),
                }
            }
        }
        candidates.dedup_by(|a, b| a.0 == b.0);
        facets.child_now = match candidates.len() {
            0 => {
                let failed = notes.iter().any(|note| note.starts_with('`'));
                if failed {
                    Facet::unavailable(child_source, notes.join("; "))
                } else if root_now.is_none() && !identity.current_work_refs.contains_key("run_ref")
                {
                    Facet::not_attempted(child_source, "neither current work placement nor a root NOW is available to find a child NOW from")
                } else {
                    Facet::absent(child_source, notes.join("; "))
                }
            }
            1 => {
                let (now_ref, source) = candidates[0].clone();
                let row = rows
                    .iter()
                    .find(|row| pick(row, &["now_ref"]).as_deref() == Some(now_ref.as_str()));
                let (facet, revision) =
                    match row.and_then(|row| pick_revision(row, &["revision"]).map(|r| (row, r))) {
                        Some((row, revision)) => (
                            Facet::present(
                                source,
                                format!(
                                    "{now_ref} @{revision}{}",
                                    pick(row, &["lifecycle"])
                                        .map(|l| format!(" [{l}]"))
                                        .unwrap_or_default()
                                ),
                                row.clone(),
                            ),
                            Some(revision),
                        ),
                        None => read_now(owners, &now_ref, source, "child"),
                    };
                identity.child_now_ref = Some(now_ref);
                identity.child_now_revision = revision;
                facet
            }
            n => Facet::ambiguous(
                child_source,
                format!("{n} child NOWs name this Position — refusing to pick one"),
                json!(candidates
                    .iter()
                    .map(|(r, _)| r.clone())
                    .collect::<Vec<_>>()),
            ),
        };
    } else {
        facets.child_now = Facet::not_attempted(
            child_source,
            "no Position resolved; a child NOW is placed per Position",
        );
    }

    // -- Peers: the Project World's Positions with their occupancy ----------
    let peers_source = "central.position.list + actuation occupancy list";
    if !standard {
        facets.peers = Facet::not_attempted(
            peers_source,
            "the lean entry leaves the roster to `aikit whoami`",
        )
        .with_next("aikit whoami");
    } else {
        let listing = owners.ctrl(
            "central.position.list",
            match &trail.project_name {
                Some(project) => json!({ "project": project }),
                None => json!({}),
            },
        );
        match listing.ok() {
            Some(data) => {
                let occupancy = match &occupancy_list {
                    Some(answer) => answer.clone(),
                    None => {
                        let answer = owners.occupancy(&["list"]);
                        if let Answer::Ok(data) = &answer {
                            trail.occupancy_list = Some(data.clone());
                        }
                        answer
                    }
                };
                let ledger: Vec<&Value> = occupancy
                    .ok()
                    .map(|data| array(data, &["positions", "occupancies", "entries"]))
                    .unwrap_or_default();
                let mut peers = Vec::new();
                let own = array(data, &["positions"])
                    .into_iter()
                    .map(|p| (p, "own"))
                    .chain(
                        array(data, &["inherited"])
                            .into_iter()
                            .map(|p| (p, "inherited")),
                    );
                for (entry, scope) in own {
                    // Central wraps each listed Position as `{record, source}`.
                    let record = entry
                        .get("record")
                        .filter(|r| r.is_object())
                        .unwrap_or(entry);
                    let Some(reference) = pick(record, &["ref", "position_ref"]) else {
                        continue;
                    };
                    if Some(&reference) == position.as_ref() {
                        continue;
                    }
                    let entry = ledger.iter().find(|entry| {
                        pick(entry, &["position_ref", "positionRef"]).as_deref()
                            == Some(reference.as_str())
                    });
                    let (state, generation_ref, agent) = match (occupancy.ok(), entry) {
                        (None, _) => ("unknown".to_owned(), None, None),
                        (Some(_), None) => ("vacant".to_owned(), None, None),
                        (Some(_), Some(entry)) => {
                            let current = entry.get("current").filter(|c| c.is_object());
                            (
                                pick(entry, &["state"]).unwrap_or_else(|| {
                                    if current.is_some() {
                                        "occupied".into()
                                    } else {
                                        "vacant".into()
                                    }
                                }),
                                current.and_then(|c| pick(c, &["generation_ref", "generationRef"])),
                                current.and_then(|c| pick(c, &["agent_ref", "agentRef"])),
                            )
                        }
                    };
                    identity.peers.insert(
                        reference.clone(),
                        match &generation_ref {
                            Some(generation_ref) => format!("{state}:{generation_ref}"),
                            None => state.clone(),
                        },
                    );
                    peers.push(json!({
                        "position_ref": reference,
                        "handle": pick(record, &["handle"]),
                        "label": pick(record, &["label"]),
                        "scope": scope,
                        "occupancy": state,
                        "generation_ref": generation_ref,
                        "agent_ref": agent,
                    }));
                }
                let occupied = peers
                    .iter()
                    .filter(|p| p["occupancy"] == "occupied")
                    .count();
                trail.peers = peers.clone();
                let invalid = data.get("invalid").cloned().unwrap_or(Value::Null);
                let mut facet = Facet::present(
                    peers_source,
                    format!(
                        "{} peer Position(s) in {}: {occupied} occupied",
                        peers.len(),
                        pick(data, &["world_ref"]).unwrap_or_else(|| "this World".into())
                    ),
                    json!({ "world_ref": data.get("world_ref"), "peers": peers, "invalid": invalid }),
                );
                if occupancy.ok().is_none() {
                    facet = facet.because(format!("occupancy unknown: {}", occupancy.describe()));
                }
                facets.peers = facet;
            }
            None => facets.peers = listing.failed_facet("central.position.list"),
        }
    }

    // -- Prepared context (Redis prepared NOW view) --------------------------
    let prepared_source = "Redis prepared NOW context";
    if !standard {
        facets.prepared_context =
            Facet::not_attempted(prepared_source, "the lean entry does not read Redis").with_next(
                "aikit now-context inspect --config-file <redis-now.json> --participant-ref <ref>",
            );
    } else {
        match reads.prepared {
            None => {
                facets.prepared_context = Facet::not_attempted(
                    prepared_source,
                    reads.prepared_absent_reason.clone().unwrap_or_else(|| {
                        format!(
                        "no Redis NOW material configured (--redis-config or {REDIS_CONFIG_VAR})"
                    )
                    }),
                )
            }
            Some(prepared) => {
                let participants: Vec<String> =
                    [identity.position_ref.clone(), identity.agent_ref.clone()]
                        .into_iter()
                        .flatten()
                        .collect();
                if participants.is_empty() {
                    facets.prepared_context = Facet::not_attempted(
                        prepared_source,
                        "no Position or Agent to key a prepared view by",
                    );
                }
                let mut errors = Vec::new();
                for participant in &participants {
                    match prepared(participant) {
                        Ok(Some(meta)) => {
                            let version = meta.get("version").and_then(Value::as_u64).unwrap_or(0);
                            let digest = pick(&meta, &["basis_digest"]).unwrap_or_default();
                            identity.prepared_context = Some(format!("v{version} {digest}"));
                            facets.prepared_context = Facet::present(
                                prepared_source,
                                format!("{participant} v{version} basis {digest}"),
                                json!({ "participant_ref": participant, "meta": meta }),
                            );
                            break;
                        }
                        Ok(None) => {}
                        Err(error) => errors.push(format!("{participant}: {}", error.message())),
                    }
                }
                if !facets.prepared_context.is_present() && !participants.is_empty() {
                    facets.prepared_context = if errors.is_empty() {
                        Facet::absent(
                            prepared_source,
                            format!("no prepared view for {}", participants.join(" or ")),
                        )
                    } else {
                        Facet::unavailable(prepared_source, errors.join("; "))
                    };
                }
            }
        }
    }

    // -- Native authority ----------------------------------------------------
    let authority_source = "central.files.read (Control/user/native-action-authority.json)";
    if !standard {
        facets.authority =
            Facet::not_attempted(authority_source, "the lean entry does not read authority")
                .with_next("aikit whoami");
    } else if let Some(root) = trail.central_root.clone() {
        let read = owners.ctrl(
            "central.files.read",
            json!({ "location": central_path_ref(&root, AUTHORITY_PATH) }),
        );
        match read.ok().and_then(|data| pick(data, &["content"])) {
            Some(content) => match serde_json::from_str::<Value>(&content) {
                Ok(document) => {
                    let grants: Vec<Value> = array(&document, &["grants"])
                        .into_iter()
                        .map(|grant| {
                            json!({
                                "principal_ref": grant.get("principal_ref"),
                                "actor_kind": grant.get("actor_kind"),
                                "scope_refs": grant.get("scope_refs"),
                                "actions": grant.get("actions").and_then(Value::as_array).map(Vec::len),
                                "expires_at_unix_seconds": grant.get("expires_at_unix_seconds"),
                            })
                        })
                        .collect();
                    let token_present =
                        std::env::var_os(NATIVE_TOKEN_VAR).is_some_and(|v| !v.is_empty());
                    let summary = grants
                        .iter()
                        .map(|g| {
                            format!(
                                "{} {} ({} actions)",
                                g["actor_kind"].as_str().unwrap_or("?"),
                                g["principal_ref"].as_str().unwrap_or("?"),
                                g["actions"].as_u64().unwrap_or(0)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("; ");
                    facets.authority = Facet::present(
                        authority_source,
                        format!("{} grant(s): {summary}", grants.len()),
                        json!({
                            "source_ref": format!("central:source:control:root:{AUTHORITY_PATH}"),
                            "revision": format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex()),
                            "scope_ref": document.get("scope_ref"),
                            "grants": grants,
                            "native_token_in_env": token_present,
                        }),
                    )
                    .because(format!(
                        "native Actions run under these grants only when the host supplies {NATIVE_TOKEN_VAR} ({} in this process); the occupant holds no grant of its own",
                        if token_present { "present" } else { "absent" }
                    ));
                }
                Err(error) => {
                    facets.authority = Facet::unavailable(
                        authority_source,
                        format!("the authority document is not JSON: {error}"),
                    )
                }
            },
            None => facets.authority = read.failed_facet(authority_source),
        }
    } else {
        facets.authority = Facet::not_attempted(authority_source, "no Central root is known");
    }

    // -- Return destination ---------------------------------------------------
    let unit_return = trail.workflow_unit.as_ref().and_then(|unit| {
        unit.get("requiredReturn")
            .or_else(|| unit.get("required_return"))
            .and_then(|r| pick(r, &["address", "ref"]))
    });
    facets.return_destination = if let Some(address) = unit_return {
        identity.return_destination = Some(address.clone());
        Facet::present(
            "factory development workflow-unit (requiredReturn)",
            address.clone(),
            json!({ "address": address, "workflow_unit_ref": identity.current_work_refs.get("workflow_unit_ref") }),
        )
    } else if let Some(child) = identity.child_now_ref.clone() {
        identity.return_destination = Some(child.clone());
        Facet::present("child NOW", child.clone(), json!({ "now_ref": child }))
            .because("current work names no Return address; the child NOW receives it")
    } else if let Some(root) = identity.root_now_ref.clone() {
        identity.return_destination = Some(root.clone());
        Facet::present("root NOW", root.clone(), json!({ "now_ref": root }))
            .because("no work Return address or child NOW; the Workcell root NOW receives it")
    } else {
        Facet::absent(
            "current work → child NOW → root NOW",
            "no current-work Return address, child NOW or root NOW resolved",
        )
        .with_next("ctrl --json action run central.now.list '{}'")
    };

    identity.occupant_generation = identity.occupant_generation.clone().or(if holds_position {
        generation.clone()
    } else {
        None
    });
    let basis_digest = identity.digest();
    let reading = InhabitationReading {
        schema: INHABITATION_READING_SCHEMA.into(),
        resolved_by,
        position_ref: position.clone(),
        occupant_generation: generation,
        depth: input.depth,
        observed_at_unix_ms: now_ms(),
        facets,
        identity,
        basis_digest,
        hot: None,
        calls: if input.depth == ReadingDepth::Full {
            owners.calls()
        } else {
            Vec::new()
        },
    };
    Joined { reading, trail }
}

/// Resolve `@handle` through `central.position.list`: exactly one listed
/// Position (own or inherited) carries it, or the facet says why not.
pub fn handle_to_ref(
    owners: &Owners<'_>,
    handle: &str,
    project: Option<&str>,
) -> std::result::Result<String, Box<Facet>> {
    let source = "central.position.list (@handle)";
    let listing = owners.ctrl(
        "central.position.list",
        project
            .map(|project| json!({ "project": project }))
            .unwrap_or_else(|| json!({})),
    );
    let Some(data) = listing.ok() else {
        let facet = listing
            .failed_facet(source)
            .with_summary(format!("@{handle}"));
        return Err(Box::new(facet));
    };
    let wanted = format!("@{handle}");
    let matches: Vec<String> = array(data, &["positions"])
        .into_iter()
        .chain(array(data, &["inherited"]))
        .map(|entry| {
            entry
                .get("record")
                .filter(|r| r.is_object())
                .unwrap_or(entry)
        })
        .filter(|record| pick(record, &["handle"]).as_deref() == Some(wanted.as_str()))
        .filter_map(|record| pick(record, &["ref", "position_ref"]))
        .collect();
    let facet = match matches.len() {
        1 => return Ok(matches[0].clone()),
        0 => Facet::absent(
            source,
            format!("no Position in this World carries {wanted}"),
        )
        .with_summary(wanted)
        .with_next(position_list_hint(project)),
        n => Facet::ambiguous(
            source,
            format!("{n} Positions carry {wanted} — refusing to pick one"),
            json!(matches),
        )
        .with_summary(wanted),
    };
    Err(Box::new(facet))
}

fn read_now(
    owners: &Owners<'_>,
    now_ref: &str,
    source: &str,
    horizon: &str,
) -> (Facet, Option<String>) {
    let read = owners.ctrl("central.now.read", json!({ "now_ref": now_ref }));
    match read.ok() {
        Some(data) => {
            let revision = data
                .get("revision")
                .and_then(|r| pick(r, &["revision"]))
                .or_else(|| pick_revision(data, &["revision"]));
            let lifecycle = data.get("record").and_then(|r| pick(r, &["lifecycle"]));
            (
                Facet::present(
                    format!("{source} + central.now.read"),
                    format!(
                        "{now_ref}{}{}",
                        revision
                            .as_deref()
                            .map(|r| format!(" @{r}"))
                            .unwrap_or_default(),
                        lifecycle
                            .as_deref()
                            .map(|l| format!(" [{l}]"))
                            .unwrap_or_default()
                    ),
                    json!({ "now_ref": now_ref, "revision": revision, "lifecycle": lifecycle, "horizon": horizon, "source": data.get("source") }),
                ),
                revision,
            )
        }
        None => (
            Facet::present(
                source,
                now_ref.to_owned(),
                json!({ "now_ref": now_ref, "horizon": horizon }),
            )
            .because(format!("revision unread: {}", read.describe())),
            None,
        ),
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn label(name: &str) -> String {
    name.replace('_', " ")
}

/// The compact human block `aikit whoami` prints.
pub fn render_text(reading: &InhabitationReading) -> String {
    let mut lines = vec![format!(
        "aikit whoami — {} · position resolved by {} · basis {}",
        reading.schema,
        reading.resolved_by.as_str(),
        reading.basis_digest
    )];
    if let Some(hot) = &reading.hot {
        lines.push(format!(
            "  hot projection: {}{}{}{}",
            hot.state,
            hot.version.map(|v| format!(" v{v}")).unwrap_or_default(),
            hot.age_ms
                .map(|a| format!(", {}s old", a / 1000))
                .unwrap_or_default(),
            hot.reason
                .as_deref()
                .map(|r| format!(" — {r}"))
                .unwrap_or_default()
        ));
    }
    let mut next = Vec::new();
    for (name, facet) in reading.facets.entries() {
        lines.push(format!(
            "  {:<19}{:<14}{}",
            label(name),
            facet.state.as_str(),
            bounded(&facet.line(), 400)
        ));
        if let Some(step) = &facet.next {
            if facet.state != FacetState::Present || name == "working_surface" {
                next.push(format!("  {:<19}{step}", label(name)));
            }
        }
    }
    if !next.is_empty() {
        lines.push("next:".into());
        lines.extend(next);
    }
    lines.join("\n")
}

/// Whether this build carries a subcommand path (e.g. `gateway who`).
pub fn verb_exists(path: &[&str]) -> bool {
    use clap::CommandFactory;
    let mut command = crate::cli::Cli::command();
    for segment in path {
        match command.find_subcommand(segment) {
            Some(found) => command = found.clone(),
            None => return false,
        }
    }
    true
}

pub const LEAN_ENTRY_MAX_CHARS: usize = 2_000;

/// The SessionStart lean entry: World, Position, occupant, work, NOW, body and
/// context pointers and the faculties — replacing the historical NOW dump.
pub fn render_lean_entry(reading: &InhabitationReading, project: Option<&str>) -> String {
    let f = &reading.facets;
    let value_line = |facet: &Facet| -> String {
        if facet.is_present() {
            facet.summary.clone().unwrap_or_default()
        } else {
            format!("{} — {}", facet.state, bounded(&facet.line(), 220))
        }
    };
    let gateway = if verb_exists(&["gateway", "who"]) {
        "aikit gateway who|send|inbox (contact other Positions)".to_owned()
    } else {
        "aikit gateway who|send|inbox — not in this AIKit build yet".to_owned()
    };
    let working_surface = match &f.working_surface.next {
        Some(next) if f.working_surface.is_present() => next.clone(),
        _ => "aikit session-space working-surface open <space> <binding>".to_owned(),
    };
    let now_pointer = match project {
        Some(project) => format!(
            "ctrl --json action run projectcentral.now.inspect '{{\"project\":\"{project}\"}}'"
        ),
        None => "ctrl --json action run central.now.list '{}'".to_owned(),
    };
    let lines = [
        format!("[O:I World inhabitation — {} · lean entry]", reading.schema),
        format!(
            "World: {} · Project World: {}",
            value_line(&f.local_world),
            value_line(&f.project_world)
        ),
        format!(
            "Position: {} (resolved by {})",
            value_line(&f.position),
            reading.resolved_by.as_str()
        ),
        format!(
            "Occupant: {} · agent {} · agency {}",
            value_line(&f.occupancy),
            f.agent.summary.as_deref().unwrap_or("-"),
            f.agency.summary.as_deref().unwrap_or("-")
        ),
        format!("Current work: {}", value_line(&f.current_work)),
        format!(
            "NOW: root {} · child {}",
            value_line(&f.root_now),
            value_line(&f.child_now)
        ),
        format!(
            "Body: {} · context: aikit compose --json; prepared: aikit now-context inspect",
            value_line(&f.body)
        ),
        format!(
            "Return: {}{}",
            value_line(&f.return_destination),
            match (&reading.position_ref, &reading.identity.occupant_generation) {
                (Some(position), Some(generation)) => format!(
                    " · attribute Returns and Communiques as actor {position} (generation {generation}), not the harness name"
                ),
                _ => String::new(),
            }
        ),
        format!(
            "Faculties: aikit whoami (full reading, peers) · aikit refocus (trace to ground) · {gateway} · skills: aikit method, aikit knowledge search · surface: {working_surface}"
        ),
        format!(
            "Historical NOW/Flow handoffs are not preloaded; read them on demand: {now_pointer}. Consequential work retrieves the governing source first."
        ),
    ];
    bounded(&lines.join("\n"), LEAN_ENTRY_MAX_CHARS)
}

// ---------------------------------------------------------------------------
// The hot World projection
// ---------------------------------------------------------------------------

/// The projection subject: the Position, else the first AgentSession id.
pub fn projection_subject(reading: &InhabitationReading, input: &JoinInput) -> Option<String> {
    reading
        .position_ref
        .clone()
        .or_else(|| input.position_flag.clone())
        .or_else(|| input.env_position.clone())
        .or_else(|| input.agent_sessions.first().map(|(_, id)| id.clone()))
}

pub fn projection_from(
    reading: &InhabitationReading,
    subject: &str,
    version: u64,
    now: u64,
) -> WorldProjection {
    WorldProjection {
        schema: WORLD_PROJECTION_SCHEMA.into(),
        subject: subject.to_owned(),
        version,
        identity: reading.identity.clone(),
        identity_digest: reading.identity.digest(),
        facet_states: reading.facets.states(),
        facet_summaries: reading
            .facets
            .entries()
            .iter()
            .filter_map(|(name, facet)| {
                facet
                    .summary
                    .as_ref()
                    .map(|summary| ((*name).to_owned(), bounded(summary, 1000)))
            })
            .collect(),
        published_at_unix_ms: now,
    }
}

/// A reading served from the hot projection: states and refs only.
pub fn reading_from_projection(projection: &WorldProjection, now: u64) -> InhabitationReading {
    let age = now.saturating_sub(projection.published_at_unix_ms);
    let source = format!(
        "redis world projection v{} ({}s old)",
        projection.version,
        age / 1000
    );
    let mut facets = InhabitationFacets::unattempted(&source, "absent from the hot projection");
    for (name, state) in &projection.facet_states {
        if let Some(slot) = facets.get_mut(name) {
            let mut facet = match state {
                FacetState::Present => Facet::present(source.clone(), "", Value::Null),
                FacetState::Absent => Facet::absent(source.clone(), ""),
                FacetState::Ambiguous => Facet::ambiguous(source.clone(), "", Value::Null),
                FacetState::Unavailable => Facet::unavailable(source.clone(), ""),
                FacetState::NotAttempted => Facet::not_attempted(source.clone(), ""),
            };
            facet.value = None;
            facet.summary = projection.facet_summaries.get(name).cloned();
            if *state != FacetState::Present {
                facet.reason = Some("state as last published; live reason: aikit whoami".into());
            }
            *slot = facet;
        }
    }
    let identity = projection.identity.clone();
    InhabitationReading {
        schema: INHABITATION_READING_SCHEMA.into(),
        resolved_by: if identity.position_ref.is_some() {
            ResolvedBy::Flag
        } else {
            ResolvedBy::None
        },
        position_ref: identity.position_ref.clone(),
        occupant_generation: identity.occupant_generation.clone(),
        depth: ReadingDepth::Standard,
        observed_at_unix_ms: projection.published_at_unix_ms,
        facets,
        basis_digest: projection.identity_digest.clone(),
        identity,
        hot: Some(HotProvenance {
            state: "served".into(),
            version: Some(projection.version),
            age_ms: Some(age),
            basis_digest: Some(projection.identity_digest.clone()),
            reason: None,
        }),
        calls: Vec::new(),
    }
}

/// Anything the Redis `world` family is reached through, so tests can use a
/// real server and the CLI a configured one.
pub trait WorldStore {
    fn version(&self, subject: &str) -> Result<u64>;
    fn publish(&self, projection: &WorldProjection, expected: u64) -> Result<u64>;
    fn read(&self, subject: &str) -> Result<Option<WorldProjection>>;
}

pub struct RedisWorldStore {
    pub store: aikit_store::now_context::RedisNowStore,
    pub secret: Option<aikit_core::SecretValue>,
}

impl WorldStore for RedisWorldStore {
    fn version(&self, subject: &str) -> Result<u64> {
        self.store.world_version(subject, self.secret.as_ref())
    }
    fn publish(&self, projection: &WorldProjection, expected: u64) -> Result<u64> {
        self.store
            .publish_world(projection, expected, self.secret.as_ref())
    }
    fn read(&self, subject: &str) -> Result<Option<WorldProjection>> {
        self.store.read_world(subject, self.secret.as_ref())
    }
}

/// Publish one reading, CAS on the stored version. An unchanged identity is
/// not republished.
pub fn publish(
    store: &dyn WorldStore,
    reading: &InhabitationReading,
    subject: &str,
) -> Result<Value> {
    let current = store.version(subject)?;
    if current > 0 {
        if let Some(existing) = store.read(subject)? {
            if existing.identity_digest == reading.identity.digest()
                && existing.facet_states == reading.facets.states()
            {
                return Ok(json!({
                    "state": "unchanged",
                    "subject": subject,
                    "version": current,
                    "basis_digest": existing.identity_digest,
                }));
            }
        }
    }
    let projection = projection_from(reading, subject, current + 1, now_ms());
    let version = store.publish(&projection, current)?;
    Ok(json!({
        "state": "published",
        "subject": subject,
        "version": version,
        "basis_digest": projection.identity_digest,
    }))
}

/// Load the Redis NOW material configuration for the World projection.
pub fn load_redis_config(path: &Path) -> Result<aikit_store::now_context::RedisNowConfig> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        AikitError::new(
            "inhabitation.redis_config_unavailable",
            format!("{}: {error}", path.display()),
        )
    })?;
    if !metadata.is_file() || metadata.len() > 64 * 1024 {
        return Err(AikitError::new(
            "inhabitation.redis_config_invalid",
            format!("{} must be a bounded regular file", path.display()),
        ));
    }
    let bytes = std::fs::read(path).map_err(|error| {
        AikitError::new(
            "inhabitation.redis_config_unavailable",
            format!("{}: {error}", path.display()),
        )
    })?;
    // Either a bare `aikit.redis-now-config/v1` or an encounter provider's
    // `now_context` block carrying one under `redis`.
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        AikitError::new(
            "inhabitation.redis_config_invalid",
            format!("{}: {error}", path.display()),
        )
    })?;
    let config = value
        .get("redis")
        .cloned()
        .or_else(|| {
            value
                .get("now_context")
                .and_then(|n| n.get("redis"))
                .cloned()
        })
        .unwrap_or(value);
    let config: aikit_store::now_context::RedisNowConfig =
        serde_json::from_value(config).map_err(|error| {
            AikitError::new(
                "inhabitation.redis_config_invalid",
                format!("{}: {error}", path.display()),
            )
        })?;
    config.validate()?;
    Ok(config)
}

pub fn open_world_store(path: &Path) -> Result<RedisWorldStore> {
    use aikit_core::secret_ref::SecretResolver;
    let config = load_redis_config(path)?;
    let secret = config
        .credential_ref
        .as_ref()
        .map(|reference| {
            aikit_adapters::secret_resolver::SuiteSecretResolver::default().resolve(reference)
        })
        .transpose()?;
    Ok(RedisWorldStore {
        store: aikit_store::now_context::RedisNowStore::new(config)?,
        secret,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use aikit_adapters::runner::ScriptedRunner;

    pub const POSITION: &str = "central:position:project:O-I:aikit-guardian";
    pub const GENERATION: &str = "actuation:generation:11111111-1111-4111-8111-111111111111";
    pub const STATE: &str = "/w/O-I/.factory/development-state.json";

    pub fn ok(data: Value) -> String {
        json!({"ok": true, "status": "success", "action": "fixture", "data": data}).to_string()
    }

    /// Every owner answering as the contract pins it.
    pub fn owner_fixture() -> ScriptedRunner {
        ScriptedRunner::new()
            .on(
                "central.world.here",
                &ok(json!({
                    "schema": "central.world-here/v1",
                    "local_world": {"ref": "control:root", "root": "/w", "identity_ref": "central:pasu:nara:local"},
                    "project_world": {"state": "present", "ref": "project:O-I", "name": "O-I", "path": "Work/O-I", "via": "cwd"},
                    "cwd": {"path": "/w/Work/O-I", "relation": "work-member"},
                    "workcells": [{"ref": "workcell:local", "declared_by": "Control/machines/current.json", "role": "current",
                                   "root_now": {"state": "present", "now_ref": "central:now:control:root:rootnow", "lifecycle": "active"}}]
                })),
            )
            .on(
                "central.position.read",
                &ok(json!({
                    "record": {"schema": "central.world-position/v1", "ref": POSITION, "revision": "r1", "slug": "aikit-guardian", "label": "AIKit Product Guardian", "handle": "@aikit-guardian", "enclosing_world_ref": "project:O-I"},
                    "source": {"ref": "central:source:project:O-I:ProjectCentral/relations/positions/aikit-guardian.json", "revision": "r1"}
                })),
            )
            .on("occupancy verify", r#"{"ok":true}"#)
            .on(
                "occupancy read",
                &json!({
                    "schema": "actuation.position-occupancy/v1",
                    "position_ref": POSITION,
                    "state": "occupied",
                    "current": {
                        "position_ref": POSITION, "generation_ref": GENERATION, "generation_ordinal": 3,
                        "kind": "fresh", "agent_ref": "agent/aikit-guardian", "agency_ref": "agency/aikit-guardian",
                        "agent_session_ref": "agent-session/s1", "harness_composition_ref": "harness/claude-code",
                        "model_ref": "model:claude-opus", "workcell_ref": "workcell:local", "began_at_unix_ms": 1, "reason": "fixture"
                    },
                    "generations": 3
                })
                .to_string(),
            )
            .on(
                "occupancy list",
                &json!({"positions": [
                    {"position_ref": POSITION, "state": "occupied", "current": {"generation_ref": GENERATION, "agent_session_ref": "agent-session/s1", "agent_ref": "agent/aikit-guardian"}},
                    {"position_ref": "central:position:project:O-I:factory-guardian", "state": "occupied", "current": {"generation_ref": "actuation:generation:f", "agent_ref": "agent/factory-guardian"}}
                ]})
                .to_string(),
            )
            .on(
                "project locate",
                &json!({"contract": "factory.project-location/v1", "statePath": STATE, "status": "ready"}).to_string(),
            )
            .on(
                "development current-work",
                &json!({
                    "schema": "factory.current-work/v1", "position_ref": POSITION, "outcome": "one",
                    "current": {"custody_ref": "factory:custody:c1", "position_ref": POSITION, "work_ref": "work:w1", "run_ref": "run:r1", "journey_ref": "journey:j1", "workflow_unit_ref": "workflow-unit:u1", "state": "in-progress"},
                    "candidates": [], "considered": 1, "basis": "one in-progress custody"
                })
                .to_string(),
            )
            .on(
                "development workflow-unit",
                &json!({"contract": "factory.workflow-unit-reading/v1", "workflowUnitRef": "workflow-unit:u1", "developmentalConcern": "Wire the joined reading", "provenance": {"subjectRevision": 4}, "requiredReturn": {"contract": "Return the reading", "address": "return:r-unit"}}).to_string(),
            )
            .on(
                "development inhabitation",
                &json!({"schema": "factory.inhabitation-reading/v1", "runs": [{"run_ref": "run:r1", "positions": [{"position_ref": POSITION, "placement_now_ref": "central:now:control:root:child1", "return_address": "return:r-unit"}]}]}).to_string(),
            )
            .on(
                "central.now.read",
                &ok(json!({"record": {"now_ref": "central:now:control:root:rootnow", "lifecycle": "active"}, "revision": {"revision": "central.content-fnv1a64/v1:10:aa"}})),
            )
            .on(
                "central.position.list",
                &ok(json!({
                    "world_ref": "project:O-I",
                    "positions": [
                        {"record": {"ref": POSITION, "handle": "@aikit-guardian"}, "source": {"revision": "r1"}},
                        {"record": {"ref": "central:position:project:O-I:factory-guardian", "handle": "@factory-guardian"}, "source": {"revision": "r1"}},
                        {"record": {"ref": "central:position:project:O-I:ql-guardian", "handle": "@ql-guardian"}, "source": {"revision": "r1"}}
                    ],
                    "inherited": [], "invalid": []
                })),
            )
            .on(
                "central.files.read",
                &ok(json!({"content": "{\"schema\":\"central.native-action-authority/v1\",\"scope_ref\":\"control:root\",\"grants\":[{\"principal_ref\":\"central:source:control:root:Control/user/identity\",\"actor_kind\":\"human\",\"token_sha256\":\"secret-hash\",\"scope_refs\":[\"control:root\"],\"actions\":[\"a\",\"b\"],\"expires_at_unix_seconds\":1}]}"})),
            )
    }

    pub fn owners(runner: &ScriptedRunner) -> Owners<'_> {
        Owners::new(
            runner,
            OwnerBins::default(),
            Some(PathBuf::from("/w")),
            Duration::from_secs(5),
            Duration::from_secs(30),
        )
    }

    pub fn env_input(depth: ReadingDepth) -> JoinInput {
        JoinInput {
            cwd: PathBuf::from("/w/Work/O-I"),
            position_flag: None,
            env_position: Some(POSITION.into()),
            env_generation: Some(GENERATION.into()),
            agent_sessions: vec![],
            depth,
            allow_session_lookup: true,
        }
    }

    #[test]
    fn env_stamped_position_joins_every_owner_into_present_facets() {
        let runner = owner_fixture();
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Standard),
            &AikitReads::default(),
        );
        let r = &joined.reading;
        assert_eq!(r.resolved_by, ResolvedBy::Env);
        let f = &r.facets;
        assert_eq!(f.local_world.state, FacetState::Present);
        assert_eq!(f.project_world.state, FacetState::Present);
        assert_eq!(f.position.state, FacetState::Present);
        assert!(f
            .position
            .summary
            .as_deref()
            .unwrap()
            .contains("AIKit Product Guardian"));
        assert_eq!(f.occupancy.state, FacetState::Present);
        assert!(f.occupancy.line().contains("verified"));
        assert_eq!(f.agent.summary.as_deref(), Some("agent/aikit-guardian"));
        assert_eq!(f.agency.summary.as_deref(), Some("agency/aikit-guardian"));
        assert_eq!(f.agent_session.summary.as_deref(), Some("agent-session/s1"));
        assert_eq!(
            f.session_space.state,
            FacetState::Absent,
            "the tenure names no SessionSpace"
        );
        assert_eq!(f.body.state, FacetState::Present);
        assert_eq!(f.workcell.state, FacetState::Present);
        assert_eq!(f.current_work.state, FacetState::Present);
        assert_eq!(f.root_now.state, FacetState::Present);
        assert_eq!(f.child_now.state, FacetState::Present);
        assert_eq!(
            r.identity.child_now_ref.as_deref(),
            Some("central:now:control:root:child1")
        );
        assert_eq!(f.peers.state, FacetState::Present);
        let peers = f.peers.value.as_ref().unwrap()["peers"].as_array().unwrap();
        assert_eq!(peers.len(), 2, "the roster excludes self and is uncapped");
        assert_eq!(
            r.identity
                .peers
                .get("central:position:project:O-I:ql-guardian")
                .map(String::as_str),
            Some("vacant")
        );
        assert_eq!(f.authority.state, FacetState::Present);
        assert!(
            !f.authority
                .value
                .as_ref()
                .unwrap()
                .to_string()
                .contains("secret-hash"),
            "token hashes never leave Central"
        );
        assert_eq!(
            f.return_destination.summary.as_deref(),
            Some("return:r-unit")
        );
        assert_eq!(f.prepared_context.state, FacetState::NotAttempted);
        assert_eq!(
            r.identity
                .current_work_refs
                .get("run_ref")
                .map(String::as_str),
            Some("run:r1")
        );
        assert!(r.identity.current_work_digest.is_some());
    }

    #[test]
    fn missing_owner_verbs_degrade_to_unavailable_with_the_exact_command() {
        let runner = ScriptedRunner::new()
            .on(
                "central.world.here",
                &json!({"ok": false, "status": "invalid_input", "action": "central.world.here", "error": {"code": "invalid_input", "message": "Unknown Action: central.world.here"}}).to_string(),
            )
            .on(
                "central.position.read",
                &json!({"ok": false, "error": {"code": "invalid_input", "message": "Unknown Action: central.position.read"}}).to_string(),
            )
            .failing("occupancy", 2, "actuation: unknown command occupancy; run actuation help")
            .failing("factory", 2, "factory: unknown development operation `current-work`");
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Standard),
            &AikitReads::default(),
        );
        let f = &joined.reading.facets;
        assert_eq!(f.local_world.state, FacetState::Unavailable);
        assert!(f
            .local_world
            .reason
            .as_deref()
            .unwrap()
            .contains("central.world.here"));
        assert!(f
            .local_world
            .reason
            .as_deref()
            .unwrap()
            .contains("Unknown Action"));
        assert_eq!(f.position.state, FacetState::Unavailable);
        assert_eq!(
            f.position.summary.as_deref(),
            Some(POSITION),
            "the stamped ref is still named"
        );
        assert_eq!(f.occupancy.state, FacetState::Unavailable);
        let reason = f.occupancy.reason.as_deref().unwrap();
        assert!(
            reason.contains("actuation occupancy read --position"),
            "{reason}"
        );
        assert!(reason.contains("unknown command occupancy"), "{reason}");
        assert_eq!(f.agent.state, FacetState::Absent);
        assert_eq!(f.current_work.state, FacetState::Unavailable);
        assert_eq!(f.return_destination.state, FacetState::Absent);
        assert_eq!(joined.reading.resolved_by, ResolvedBy::Env);
    }

    #[test]
    fn a_superseded_generation_is_not_the_occupant() {
        let runner = owner_fixture().failing(
            "occupancy verify",
            3,
            r#"{"code":"occupancy.superseded","fact":"generation g is superseded","consequence":"nothing was verified","action":"actuation occupancy read --position P --json"}"#,
        );
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Lean),
            &AikitReads::default(),
        );
        let f = &joined.reading.facets;
        assert_eq!(f.occupancy.state, FacetState::Absent);
        assert!(f
            .occupancy
            .reason
            .as_deref()
            .unwrap()
            .contains("occupancy.superseded"));
        assert_eq!(joined.reading.identity.occupant_generation, None);
    }

    #[test]
    fn agent_session_resolution_matches_exactly_one_open_tenure_or_refuses() {
        let runner = owner_fixture();
        let mut input = env_input(ReadingDepth::Lean);
        input.env_position = None;
        input.env_generation = None;
        input.agent_sessions = vec![("--agent-session".into(), "agent-session/s1".into())];
        let joined = join(&owners(&runner), &input, &AikitReads::default());
        assert_eq!(joined.reading.resolved_by, ResolvedBy::AgentSession);
        assert_eq!(joined.reading.position_ref.as_deref(), Some(POSITION));
        assert_eq!(
            joined.reading.occupant_generation.as_deref(),
            Some(GENERATION)
        );

        let ambiguous = owner_fixture().on(
            "occupancy list",
            &json!({"positions": [
                {"position_ref": "p:a", "current": {"generation_ref": "g:a", "agent_session_ref": "agent-session/s1"}},
                {"position_ref": "p:b", "current": {"generation_ref": "g:b", "agent_session_ref": "agent-session/s1"}}
            ]})
            .to_string(),
        );
        let joined = join(&owners(&ambiguous), &input, &AikitReads::default());
        assert_eq!(joined.reading.facets.position.state, FacetState::Ambiguous);
        assert_eq!(joined.reading.position_ref, None);

        input.agent_sessions.clear();
        let fresh = owner_fixture();
        let joined = join(&owners(&fresh), &input, &AikitReads::default());
        assert_eq!(joined.reading.resolved_by, ResolvedBy::None);
        assert_eq!(joined.reading.facets.position.state, FacetState::Absent);
        assert!(
            !fresh
                .call_lines()
                .iter()
                .any(|line| line.contains("occupancy list")),
            "no identifier, no list call"
        );
    }

    #[test]
    fn an_unallocated_root_now_names_the_ensure_action_and_never_runs_it() {
        let runner = owner_fixture().on(
            "central.world.here",
            &ok(json!({
                "local_world": {"ref": "control:root", "root": "/w"},
                "project_world": {"state": "present", "ref": "project:O-I", "name": "O-I", "path": "Work/O-I", "via": "cwd"},
                "workcells": [{"ref": "workcell:local", "role": "current",
                               "root_now": {"state": "absent", "now_ref": "central:now:control:root:x", "reason": "no Workcell root NOW is allocated; central.now.workcell-root ensures it"}}]
            })),
        );
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Lean),
            &AikitReads::default(),
        );
        let root = &joined.reading.facets.root_now;
        assert_eq!(root.state, FacetState::Absent);
        assert!(root
            .next
            .as_deref()
            .unwrap()
            .contains("central.now.workcell-root"));
        assert!(
            !runner
                .call_lines()
                .iter()
                .any(|l| l.contains("central.now.workcell-root")),
            "a reading never ensures"
        );
    }

    #[test]
    fn a_child_now_is_found_among_the_root_children_by_position() {
        let runner = owner_fixture()
            .failing("development inhabitation", 2, "factory: unknown development operation `inhabitation`")
            .on(
                "central.now.children",
                &ok(json!({"schema": "central.now-children/v1", "children": [
                    {"now_ref": "central:now:project:O-I:other", "participant_refs": ["central:position:project:O-I:ql-guardian"], "revision": "rev-o"},
                    {"now_ref": "central:now:project:O-I:mine", "participant_refs": [POSITION], "revision": "rev-m", "lifecycle": "active"}
                ]})),
            );
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Lean),
            &AikitReads::default(),
        );
        assert_eq!(joined.reading.facets.child_now.state, FacetState::Present);
        assert_eq!(
            joined.reading.identity.child_now_ref.as_deref(),
            Some("central:now:project:O-I:mine")
        );
        assert_eq!(
            joined.reading.identity.child_now_revision.as_deref(),
            Some("rev-m")
        );
    }

    #[test]
    fn a_handle_resolves_through_the_listing_or_says_why_not() {
        let runner = owner_fixture();
        let mut input = env_input(ReadingDepth::Lean);
        input.env_position = None;
        input.env_generation = None;
        input.position_flag = Some("@aikit-guardian".into());
        let joined = join(&owners(&runner), &input, &AikitReads::default());
        assert_eq!(joined.reading.position_ref.as_deref(), Some(POSITION));
        assert_eq!(joined.reading.facets.position.state, FacetState::Present);

        input.position_flag = Some("@nobody".into());
        let joined = join(&owners(&owner_fixture()), &input, &AikitReads::default());
        assert_eq!(joined.reading.position_ref, None);
        assert_eq!(joined.reading.facets.position.state, FacetState::Absent);
        assert_eq!(
            joined.reading.facets.position.summary.as_deref(),
            Some("@nobody")
        );
    }

    #[test]
    fn ambiguous_current_work_is_carried_not_resolved() {
        let runner = owner_fixture().on(
            "development current-work",
            &json!({"schema": "factory.current-work/v1", "outcome": "ambiguous", "candidates": [{"work_ref": "a"}, {"work_ref": "b"}], "considered": 2, "basis": "2 distinct work nodes"}).to_string(),
        );
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Lean),
            &AikitReads::default(),
        );
        assert_eq!(
            joined.reading.facets.current_work.state,
            FacetState::Ambiguous
        );
        assert!(joined.reading.identity.current_work_refs.is_empty());
    }

    #[test]
    fn an_exhausted_budget_marks_later_calls_not_attempted() {
        let runner = owner_fixture();
        let owners = Owners::new(
            &runner,
            OwnerBins::default(),
            None,
            Duration::from_secs(1),
            Duration::ZERO,
        );
        let joined = join(
            &owners,
            &env_input(ReadingDepth::Standard),
            &AikitReads::default(),
        );
        assert_eq!(
            joined.reading.facets.local_world.state,
            FacetState::NotAttempted
        );
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn lean_entry_is_bounded_and_points_instead_of_dumping() {
        let runner = owner_fixture();
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Lean),
            &AikitReads::default(),
        );
        let lean = render_lean_entry(&joined.reading, Some("O-I"));
        assert!(
            lean.chars().count() <= LEAN_ENTRY_MAX_CHARS,
            "{}",
            lean.len()
        );
        assert!(lean.contains("Position: central:position:project:O-I:aikit-guardian"));
        assert!(lean.contains("aikit whoami"));
        assert!(lean.contains("aikit refocus"));
        assert!(lean.contains("aikit gateway who|send|inbox"));
        assert!(lean.contains("projectcentral.now.inspect"));
        assert!(
            !runner
                .call_lines()
                .iter()
                .any(|l| l.contains("central.position.list")),
            "lean reads no roster"
        );
    }

    /// Publish → delete the keys → rebuild from the owners: the identity refs
    /// come back identical, because every one is recomputed from its owner.
    #[test]
    fn redis_loss_and_rebuild_preserve_semantic_identity() {
        let Ok(address) = std::env::var("AIKIT_TEST_REDIS_ADDR") else {
            eprintln!("skipping: AIKIT_TEST_REDIS_ADDR is not set");
            return;
        };
        let config = aikit_store::now_context::RedisNowConfig {
            schema: aikit_store::now_context::NOW_REDIS_CONFIG_SCHEMA.into(),
            address,
            database: 0,
            key_prefix: format!("aikit-world-test-{}", ulid::Ulid::generate()),
            username: None,
            credential_ref: None,
            allow_remote: false,
            connect_timeout_ms: 1000,
            io_timeout_ms: 1000,
            prepared_ttl_seconds: 3600,
            coordination_retention_seconds: 3600,
        };
        let world = RedisWorldStore {
            store: aikit_store::now_context::RedisNowStore::new(config).unwrap(),
            secret: None,
        };
        let input = env_input(ReadingDepth::Standard);
        let live = join(&owners(&owner_fixture()), &input, &AikitReads::default()).reading;
        let subject = projection_subject(&live, &input).unwrap();
        assert_eq!(subject, POSITION);
        let published = publish(&world, &live, &subject).unwrap();
        assert_eq!(published["state"], "published");
        assert_eq!(published["version"], 1);
        assert_eq!(
            publish(&world, &live, &subject).unwrap()["state"],
            "unchanged"
        );
        let hot = reading_from_projection(&world.read(&subject).unwrap().unwrap(), now_ms());
        assert_eq!(hot.identity, live.identity);

        world.store.delete_world(&subject, None).unwrap();
        assert!(
            world.read(&subject).unwrap().is_none(),
            "Redis lost the projection"
        );

        let rebuilt = join(&owners(&owner_fixture()), &input, &AikitReads::default()).reading;
        let republished = publish(&world, &rebuilt, &subject).unwrap();
        assert_eq!(republished["version"], 1);
        let restored = world.read(&subject).unwrap().unwrap();
        assert_eq!(restored.identity, live.identity);
        assert_eq!(restored.identity_digest, live.identity.digest());
        assert_eq!(restored.facet_states, live.facets.states());
        world.store.delete_world(&subject, None).unwrap();
    }

    #[test]
    fn the_hot_projection_round_trips_identity_and_states() {
        let runner = owner_fixture();
        let joined = join(
            &owners(&runner),
            &env_input(ReadingDepth::Standard),
            &AikitReads::default(),
        );
        let projection = projection_from(&joined.reading, POSITION, 1, 10);
        projection.validate().unwrap();
        let hot = reading_from_projection(&projection, 5_010);
        assert_eq!(hot.identity, joined.reading.identity);
        assert_eq!(hot.facets.states(), joined.reading.facets.states());
        assert_eq!(hot.hot.as_ref().unwrap().age_ms, Some(5_000));
        assert!(
            serde_json::to_string(&projection).unwrap().len() < 16 * 1024,
            "refs only"
        );
    }
    #[test]
    fn current_work_refs_read_factorys_real_current_work_shape() {
        // Shape emitted by `factory development current-work` (EpiLogos/Factory#260).
        let reading = json!({
            "schema": "factory.current-work/v1",
            "outcome": "one",
            "current": {
                "node_ref": "github:EpiLogos/Factory#261",
                "kind": "work",
                "work_refs": ["github:EpiLogos/Factory#261"],
                "journey_refs": ["journey:01M38BNMZ0YVZ3DQ9VSDKYEZKY"],
                "custody_refs": ["factory:custody:01a0d080"],
                "attempt_refs": []
            },
            "candidates": [{
                "source": "custody", "source_ref": "factory:custody:01a0d080",
                "resolution": "resolved", "node_ref": "github:EpiLogos/Factory#261",
                "work_ref": "github:EpiLogos/Factory#261", "run_ref": "run:01M38BNMZ05K9HV7TJZ5QEW5JZ",
                "journey_ref": "journey:01M38BNMZ0YVZ3DQ9VSDKYEZKY", "status": "in-progress"
            }],
            "considered": 1
        });
        let refs = current_work_refs(&reading);
        assert_eq!(refs["work_ref"], "github:EpiLogos/Factory#261");
        assert_eq!(refs["run_ref"], "run:01M38BNMZ05K9HV7TJZ5QEW5JZ");
        assert_eq!(refs["journey_ref"], "journey:01M38BNMZ0YVZ3DQ9VSDKYEZKY");
        assert_eq!(refs["custody_ref"], "factory:custody:01a0d080");
        assert_eq!(refs["state"], "in-progress");

        // A different work node changes the digest the Refocus transition reads.
        let mut moved = reading.clone();
        moved["current"]["node_ref"] = json!("github:EpiLogos/Factory#262");
        moved["candidates"][0]["node_ref"] = json!("github:EpiLogos/Factory#262");
        moved["candidates"][0]["work_ref"] = json!("github:EpiLogos/Factory#262");
        assert_ne!(current_work_digest(&reading), current_work_digest(&moved));
    }
}
