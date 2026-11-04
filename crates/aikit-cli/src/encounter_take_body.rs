//! A Repeat's take with the session's own agent as its body (O-I manual
//! §19.1, The Launch panel: "Each run is a take"; O-I #795). Actuation's
//! comparison run makes the takes; each take runs a body over Actuation's
//! `agent-jsonl` protocol (actuation-research `sdk.rs`), and this module is
//! that body for an AIKit session: `aikit session-space encounter-take-body
//! --session S`.
//!
//! The body is a client of the resident encounter owner, as every other
//! turn-taker is (`encounter_resident_turn.rs`): it opens a fresh resident of
//! the session's agent — the provider the session's own native body opened
//! on (the bound task's own agent, not its Workcell launcher) — in the take's
//! World, and never touches the session's own resident. Over the protocol:
//!
//! - `preflight {configuration}` checks the configuration names this
//!   session's agent and provider, opens the fresh resident with the World as
//!   its cwd, holds the configuration's model (`ModelSelect`) and applies the
//!   take's level: a model is the model; an effort is `ModelSelect`'s
//!   `provider_reasoning_effort`; Tool calls (Ask, Session, Always) is the
//!   take's answer to the harness's requests to use a tool, given in the
//!   harness's own words on each turn as the task executor gives it
//!   (`tool_calls_answer`). `inherit` applies nothing. It answers what was
//!   applied and confirmed — `{ready, provider, model, agent, level}` — or
//!   refuses in words. A Skillset level (`set`, or `set+member`) is a set of
//!   skills, or that set with one proposed revision of one member in it
//!   (§19.1; §19.7 Refining a skill): before the resident opens, the set is
//!   carried into the take's World where its harness reads its project's
//!   skills (pi `.pi/skills`), the proposed revision in place of the member's
//!   revision in use, so it is in effect for that take only and nothing in
//!   the person's Home or project changes; what was carried is read back from
//!   the World, and the take is kept with the proposal as its Test. A budget
//!   has no owner operation on a fresh resident yet, so a take that varies one
//!   is refused before anything opens. Autonomy is retired: Tool calls took
//!   its place.
//! - `turn {turn: {iteration, prompt}}` takes one turn of the resident and
//!   answers `{status, output, usage}`: the agent acts in its World in its
//!   own loop, and Actuation reads the stop condition between turns.
//! - `complete {request: {system, prompt}}` takes one turn with the projected
//!   request as its text and answers `{output, usage}` (a QL condition's
//!   turns).
//! - `finalize` releases the resident: an idle, exactly identified native
//!   body, its canonical history kept; and closes the take's NOW.
//!
//! Each take keeps its own NOW (§19.1: "that NOW names the takes it was run
//! beside, so a take's close is read against its siblings' closes"): once the
//! resident is open and its level applied, Central allocates the take a child
//! of the session's task NOW ([`take_now_request`]) — its task the take's own,
//! under the task's; its sources the takes Actuation says it runs beside
//! (`configuration.takes`, the Repeat's family); its participant the take's
//! session — and `finalize` closes it, which is the take's close. A session
//! that holds no prepared task has no NOW for a take's to hang from, and the
//! take says so (`now.none`).
//!
//! A turn that asks for a tool's permission is cancelled — nobody answers the
//! provider's consent for the person in a take — unless the take varies Tool
//! calls to Session or Always, the person's own answer: then the harness's
//! own option for it is given, never a deny. Usage is the provider's own
//! report, never estimated.
//!
//! The run the takes share, in Actuation's own words (research
//! `comparison.rs` `run_with`), is composed here too ([`take_run`]) and read
//! with the session's settings (`encounter_task_settings.rs`): the session's
//! bound task as a provided task — its id the task's ref, its prompt the
//! task's name as it stands (the text a Loop's run resubmits), no success
//! conditions and no test, because AIKit keeps neither — and this body as an
//! `agent-jsonl` process.
use crate::encounter_resident_turn::{total_tokens, ResidentOwner, TurnAnswer, TurnWords};
use crate::encounter_service::{EncounterRequest, EncounterService};
use aikit_adapters::central_placement::{AllocatedCentralTask, CentralTaskRequest};
use aikit_core::session_space::SessionSpaceRef;
use aikit_core::session_space_application::{
    SessionSpaceAgentAttachmentIntent, SessionSpaceMutation,
};
use aikit_core::{AikitError, ResourceRef, Result};
use aikit_store::{AikitHome, SessionSpaceApplicationStore};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Actuation's protocol for an agent body.
pub const PROTOCOL: &str = "agent-jsonl";
/// The session time Actuation gives one take's body, and its process bound.
pub const SESSION_TIMEOUT_MS: u64 = 30 * 60 * 1000;
/// How long one turn of a take may run before it is cancelled.
pub const TURN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The most protocol bytes one take's body may write.
const OUTPUT_LIMIT: u64 = 16 * 1024 * 1024;
/// A take's turns and capability calls: an agent body acts itself, so its
/// calls are its own; these bound a QL condition's run.
const MAX_STEPS: u64 = 8;
const MAX_CALLS: u64 = 64;

/// Why a session's takes have no run, in AIKit's words.
pub const NEVER_OPENED: &str = "The session's agent has never opened: a take runs a fresh resident of the agent the session opened on, and this session's journal names none. Open the session once, then press Repeat again.";
pub const NO_MODEL: &str = "The session's agent has opened, but AIKit observed no model on it: a take holds the session's model, and this agent advertised none.";
/// Why a take keeps no NOW, in AIKit's words.
pub const NO_TASK_NOW: &str = "The session holds no prepared task, so this take keeps no NOW of its own: a take's NOW hangs from its task's (O-I manual §19.1), and is allocated once the session's task is prepared.";

const TAKE_TURN: TurnWords<'static> = TurnWords {
    session: "the take's session",
    act: "a take",
};

fn failure(message: impl Into<String>) -> AikitError {
    AikitError::new("encounter.take_body", message)
}

/// The agent a session opens on, as its own journal recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAgent {
    /// The configured encounter provider its native body opened with.
    pub provider: String,
    /// The provider model it was last observed on.
    pub model: Option<String>,
}

/// The process a take's body runs as: this AIKit, against this home, at its
/// resident owner's socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeHost {
    pub program: PathBuf,
    /// What precedes the verb: `session-space` for `aikit`, nothing for the
    /// standalone `aikit-session-space`.
    pub args_prefix: Vec<String>,
    /// Actuation clears a body's environment: the home and its context travel
    /// explicitly.
    pub environment: BTreeMap<String, String>,
    pub socket: PathBuf,
}

impl TakeHost {
    /// This AIKit as it invokes itself (`session_space_verb_prefix`, as the
    /// resident spawn and the task launcher do).
    pub fn of_this_process(home: &AikitHome) -> Result<Self> {
        let program = std::env::current_exe()
            .map_err(|error| failure(format!("AIKit cannot name its own executable: {error}")))?;
        let mut environment = BTreeMap::new();
        environment.insert("AIKIT_HOME".into(), home.root().display().to_string());
        for key in ["HOME", "AIKIT_CONTEXT_ID", "AIKIT_ISOLATION"] {
            if let Ok(value) = std::env::var(key) {
                environment.insert(key.into(), value);
            }
        }
        Ok(Self {
            program,
            args_prefix: crate::session_space_verb_prefix()
                .map(|prefix| vec![prefix.to_owned()])
                .unwrap_or_default(),
            environment,
            socket: crate::encounter_service::socket_path(home),
        })
    }
}

/// The provider a session's agent is: the one its native body opened with,
/// unless that is the bound task's Workcell launcher, which wraps the task's
/// own agent (`request.provider`) — then that agent, where it is configured.
pub(crate) fn agent_provider(
    opened: &str,
    task: &Value,
    configured: impl Fn(&str) -> bool,
) -> String {
    if task["launcher"]["id"].as_str() == Some(opened) {
        if let Some(inner) = task["request"]["provider"]["id"].as_str() {
            if configured(inner) {
                return inner.to_owned();
            }
        }
    }
    opened.to_owned()
}

/// The run a session's takes share, in Actuation's words, or why there is
/// none: `{run, basis}` or `{refused}`. `task` is the settings reading's
/// bound task.
pub fn take_run(
    host: &TakeHost,
    session: &ResourceRef,
    task: &Value,
    agent: Option<&SessionAgent>,
) -> Value {
    let Some(agent) = agent else {
        return json!({"refused": NEVER_OPENED});
    };
    let Some(model) = &agent.model else {
        return json!({"refused": NO_MODEL});
    };
    let (Some(task_ref), Some(name)) = (task["task_ref"].as_str(), task["name"].as_str()) else {
        return json!({"refused": "No task is bound to this session: a take runs the session's own task."});
    };
    let mut args = host.args_prefix.clone();
    args.extend([
        "encounter-take-body".to_owned(),
        "--session".to_owned(),
        session.as_str().to_owned(),
        "--socket".to_owned(),
        host.socket.display().to_string(),
    ]);
    json!({
        "run": {
            "task": {"id": task_ref, "prompt": name, "successConditions": []},
            "body": {
                "protocol": PROTOCOL,
                "fixture_provider": false,
                "session_timeout_ms": SESSION_TIMEOUT_MS,
                "configuration": {"provider": agent.provider, "model": model, "agent": session.as_str()},
                "process": {
                    "program": host.program,
                    "args": args,
                    "cwd": "/",
                    "environment": host.environment,
                    "timeout_ms": SESSION_TIMEOUT_MS,
                    "output_limit": OUTPUT_LIMIT,
                },
            },
            "max_steps": MAX_STEPS,
            "max_calls": MAX_CALLS,
        },
        "basis": {
            "host_id": "aikit-encounter-take-body",
            "host_revision": env!("CARGO_PKG_VERSION"),
            "benchmark_revision": "own-task",
            "runner_revision": env!("CARGO_PKG_VERSION"),
            // Held, and stated: AIKit applies no network policy to a take
            // (the agent's provider keeps its own) and keeps no review
            // contract for takes. Null would read as unknown, and a matched
            // set would read unmatched for want of evidence.
            "review_contract_revision": "none",
            "network_policy_digest": "none",
        },
    })
}

/// A take's own session: one SessionSpace per set of takes (the trial's
/// trace without its `:trial:N`), one agent session per take, both named
/// from the session they take after, so a take never lands in another's.
pub fn take_session(own: &ResourceRef, trace: &str) -> (SessionSpaceRef, ResourceRef) {
    let set = trace.split(":trial:").next().unwrap_or(trace);
    let key = |text: &str| {
        blake3::hash(format!("{}\n{text}", own.as_str()).as_bytes()).to_hex()[..24].to_owned()
    };
    (
        SessionSpaceRef::parse(&format!("session-space/takes-{}", key(set)))
            .expect("a hashed ref is a valid ref"),
        ResourceRef::parse(format!("agent-session/take-{}", key(trace)))
            .expect("a hashed ref is a valid ref"),
    )
}

/// Central's own words for a refusal of its owner, where its receipt names
/// them; else AIKit's.
fn central_words(error: &AikitError) -> String {
    error
        .details()
        .get("native_receipt")
        .and_then(|receipt| serde_json::from_str::<Value>(receipt).ok())
        .and_then(|receipt| receipt["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| error.message().to_owned())
}

/// A take's own task: under its task's, named by its trace.
fn take_task_ref(task: &ResourceRef, trace: &str) -> std::result::Result<ResourceRef, String> {
    ResourceRef::parse(format!("{task}:take:{trace}")).map_err(|error| {
        format!(
            "A take's NOW cannot be named for {trace}: {}",
            error.message()
        )
    })
}

/// The NOW a take keeps (O-I manual §19.1: "Each take keeps its own NOW, and
/// that NOW names the takes it was run beside, so a take's close is read
/// against its siblings' closes"): a child of the session's task NOW, on the
/// same Central, project and Workcell; its task the take's own; its sources
/// the takes it was run beside, by their own tasks, which name their NOWs; its
/// participant the take's session. `takes` is the Repeat's family as
/// Actuation names it, this take among it; empty, the take is run beside none.
pub(crate) fn take_now_request(
    task: &AllocatedCentralTask,
    trace: &str,
    takes: &[String],
    take: &ResourceRef,
    level: &Value,
) -> std::result::Result<CentralTaskRequest, String> {
    let parent = task.allocation["now_ref"]
        .as_str()
        .filter(|now| !now.trim().is_empty())
        .ok_or_else(|| "The session's task names no NOW for a take's to hang from".to_owned())?;
    if !takes.is_empty() && !takes.iter().any(|other| other == trace) {
        return Err(format!(
            "The take {trace} is not among the takes its Repeat names"
        ));
    }
    let task_ref = &task.request.task_ref;
    let beside = takes
        .iter()
        .filter(|other| *other != trace)
        .map(|other| take_task_ref(task_ref, other))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let varies = match (level["variable"].as_str(), level["value"].as_str()) {
        (Some(variable), Some(value)) => format!(", varying {variable} {value}"),
        _ => String::new(),
    };
    Ok(CentralTaskRequest {
        ctrl_bin: task.request.ctrl_bin.clone(),
        central_root: task.request.central_root.clone(),
        project: task.request.project.clone(),
        task_ref: take_task_ref(task_ref, trace)?,
        purpose: format!(
            "A take of {task_ref} ({trace}){varies}: run beside {} other takes of its Repeat, named in its sources, so its close is read against theirs (O-I manual §19.1).",
            beside.len()
        ),
        participant_refs: vec![take.clone()],
        source_refs: beside,
        parent_now_ref: Some(
            ResourceRef::parse(parent).map_err(|error| error.message().to_owned())?,
        ),
        workcell_ref: task.request.workcell_ref.clone(),
        work_refs: vec![],
    })
}

/// The level a take varies, as the body applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Level {
    None,
    /// The model is the configuration's model: Actuation sets it so.
    Model,
    Effort(String),
    /// Tool calls' word: the take's answer to the harness's requests.
    ToolCalls(String),
    /// A set of skills, or that set with one member's proposed revision.
    Skillset {
        set: String,
        member: Option<String>,
    },
    Inherit,
}

fn level_of(level: &Value, model: &str) -> std::result::Result<Level, String> {
    if level.is_null() {
        return Ok(Level::None);
    }
    let (Some(variable), Some(value)) = (level["variable"].as_str(), level["value"].as_str())
    else {
        return Err("A take's level is {variable, value}".into());
    };
    match variable {
        "model" if value == model => Ok(Level::Model),
        "model" => Err(format!(
            "The take varies the model to {value}, but its configuration names {model}"
        )),
        _ if value == "inherit" && matches!(variable, "effort" | "tool_calls") => Ok(Level::Inherit),
        "effort" => Ok(Level::Effort(value.to_owned())),
        "tool_calls" if crate::encounter_task_executor::TOOL_CALLS.contains(&value) => {
            Ok(Level::ToolCalls(value.to_owned()))
        }
        "tool_calls" => Err(format!(
            "A Tool calls level is one of {}; the take was given {value}",
            crate::encounter_task_executor::TOOL_CALLS.join(", ")
        )),
        "autonomy" => Err(crate::encounter_task_executor::AUTONOMY_RETIRED.to_owned()),
        "skillset" => {
            let (set, member) = match value.split_once('+') {
                Some((set, member)) => (set.trim(), Some(member.trim())),
                None => (value.trim(), None),
            };
            if set.is_empty() || member.is_some_and(str::is_empty) {
                return Err(format!(
                    "A Skillset level names a set, or a set and the one member whose proposed revision it carries (set+member); the take was given {value}"
                ));
            }
            Ok(Level::Skillset {
                set: set.to_owned(),
                member: member.map(str::to_owned),
            })
        }
        "budget" => Err(format!(
            "A take's body applies a model, an effort, Tool calls or a set to the session's agent; a {variable} has no owner operation on a fresh resident yet, so this take cannot vary it"
        )),
        other => Err(format!("A take's body applies no {other} level")),
    }
}

/// What a Skillset take carried into its World, read back from it.
struct Carried {
    reading: Value,
    /// The member whose proposed revision the take carried.
    proposed: Option<String>,
}

/// Carry a set into the take's World where its harness reads its project's
/// skills: each member's revision in use, and in place of `member`'s its
/// proposed revision. The set is the skill tree there for this take: what
/// stood at the prefix in the World is replaced (a link is unlinked, never
/// followed). A member AIKit cannot carry as it stands is withheld with
/// AIKit's own reason. Each carried tree is read back from the World.
fn carry_set(
    home: &AikitHome,
    world: &Path,
    harness: &str,
    prefix: &str,
    set: &str,
    member: Option<&str>,
) -> std::result::Result<Carried, String> {
    use aikit_core::catalog::Catalog;
    let relative = Path::new(prefix);
    if relative.is_absolute()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "{harness}'s skill tree {prefix} is not a path inside the take's World"
        ));
    }
    let loaded = aikit_store::skillsets::load(home, set)
        .map_err(|error| format!("AIKit holds no set {set}: {}", error.message()))?;
    let members = loaded.all_members();
    let refinement = match member {
        None => None,
        Some(member) => {
            if !members.iter().any(|id| id.to_string() == member) {
                return Err(format!(
                    "{member} is not a member of {set}: a Skillset take carries one proposed revision of one of its set's members"
                ));
            }
            let refinement = crate::skill_sources::refinement(home, member).map_err(|error| {
                if error.code() == "refinement.none" {
                    format!("A Skillset take carries a member's proposed revision, and no proposed revision of {member} stands")
                } else {
                    format!("The proposed revision of {member} could not be read: {}", error.message())
                }
            })?;
            if refinement.stale {
                return Err(format!(
                    "The proposed revision of {member} was drafted on a revision no longer in use: discard it and propose again on the one in use"
                ));
            }
            Some(refinement)
        }
    };
    let service = crate::app::Service::open_user_scope(home.clone())
        .map_err(|error| format!("AIKit's catalogue could not be read: {}", error.message()))?;
    let catalog = service.snapshot();
    let view = service.resolved();
    let mut carried: Vec<(String, PathBuf, Value)> = Vec::new();
    let mut withheld = Vec::new();
    for id in &members {
        let withhold = |reason: String| json!({"id": id.to_string(), "reason": reason});
        if let Some(reason) = view.unavailable_reason(id) {
            withheld.push(withhold(reason.describe()));
            continue;
        }
        let Some(capsule) = catalog.get(id) else {
            withheld.push(withhold("not present in any registry".into()));
            continue;
        };
        let aikit_core::capsule::Payload::Skill(section) = &capsule.payload else {
            withheld.push(withhold(format!(
                "{} is not a skill",
                capsule.kind.as_str()
            )));
            continue;
        };
        let name = if section.export_name.is_empty() {
            id.leaf().to_owned()
        } else {
            section.export_name.clone()
        };
        let proposed = refinement
            .as_ref()
            .filter(|refinement| refinement.member == id.to_string());
        let (payload, revision) = match proposed {
            Some(refinement) => (
                crate::skill_sources::proposed_payload(home, refinement),
                refinement.proposed.revision.clone(),
            ),
            None => match &capsule.root {
                Some(root) => (
                    root.join(&section.root),
                    capsule
                        .revision
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default(),
                ),
                None => {
                    withheld.push(withhold("has no payload on this machine".into()));
                    continue;
                }
            },
        };
        if carried.iter().any(|(other, _, _)| *other == name) {
            return Err(format!(
                "Two members of {set} are the skill {name}; a harness's skill tree holds one skill of a name"
            ));
        }
        carried.push((
            name.clone(),
            payload,
            json!({"id": id.to_string(), "name": name, "revision": revision, "proposed": proposed.is_some()}),
        ));
    }
    let root = world.join(relative);
    match std::fs::symlink_metadata(&root) {
        Ok(meta) if meta.file_type().is_symlink() || meta.is_file() => std::fs::remove_file(&root),
        Ok(_) => std::fs::remove_dir_all(&root),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
    .and_then(|_| std::fs::create_dir_all(&root))
    .map_err(|error| format!("The take's World could not take the set at {prefix}: {error}"))?;
    let mut rows = Vec::new();
    for (name, payload, row) in carried {
        let into = root.join(&name);
        let read = |path: &Path| {
            crate::skill_sources::tree_files(path).map_err(|error| {
                format!("{name} could not be read for the take: {}", error.message())
            })
        };
        let wanted = read(&payload)?;
        crate::skill_sources::copy_tree(&payload, &into).map_err(|error| {
            format!(
                "{name} could not be carried into the take's World: {}",
                error.message()
            )
        })?;
        if read(&into)? != wanted {
            return Err(format!(
                "The take's World does not read back {name} as it was carried"
            ));
        }
        rows.push(row);
    }
    let proposed = refinement.as_ref().map(|refinement| {
        json!({"member": refinement.member, "revision": refinement.proposed.revision,
            "in_use": refinement.in_use.revision, "source": refinement.source})
    });
    let mut reading = json!({"set": set, "harness": harness, "root": prefix,
        "members": rows, "withheld": withheld});
    if let Some(proposed) = proposed {
        reading["proposed"] = proposed;
    }
    Ok(Carried {
        reading,
        proposed: refinement.map(|refinement| refinement.member),
    })
}

/// The resident a take opened: its own session, exactly identified.
#[derive(Debug, Clone)]
struct Opened {
    session: ResourceRef,
    native_session_id: String,
    generation: String,
}

/// One take's body, over the resident owner.
#[cfg(unix)]
pub struct TakeBody {
    home: AikitHome,
    own: ResourceRef,
    owner: ResidentOwner,
    turn_timeout: Duration,
    preflighted: bool,
    opened: Option<Opened>,
    turns: u64,
    /// The take's Tool calls, where it varies them: its answer to the
    /// harness's requests on each turn.
    tool_calls: Option<String>,
    /// The take's own NOW, once Central has allocated it; closed at finalize.
    now: Option<AllocatedCentralTask>,
}

#[cfg(unix)]
impl TakeBody {
    pub fn new(home: AikitHome, own: ResourceRef, socket: PathBuf, turn_timeout: Duration) -> Self {
        Self {
            home,
            own,
            owner: ResidentOwner::at(socket),
            turn_timeout,
            preflighted: false,
            opened: None,
            turns: 0,
            tool_calls: None,
            now: None,
        }
    }

    /// Answer the protocol line by line until `finalize` or the end of input;
    /// a resident still open at the end is released.
    pub fn serve(mut self, input: impl BufRead, mut output: impl Write) -> Result<()> {
        let write = |output: &mut dyn Write, line: Value| -> Result<()> {
            writeln!(output, "{line}")
                .and_then(|_| output.flush())
                .map_err(|error| failure(format!("the protocol's output closed: {error}")))
        };
        for line in input.lines() {
            let line =
                line.map_err(|error| failure(format!("the protocol's input failed: {error}")))?;
            if line.trim().is_empty() {
                continue;
            }
            let message: Value = match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(error) => {
                    write(
                        &mut output,
                        json!({"type":"response","id":Value::Null,"success":false,
                        "data":{"error":format!("An unreadable protocol line: {error}")}}),
                    )?;
                    continue;
                }
            };
            let kind = message["type"].as_str().unwrap_or_default().to_owned();
            let answer = match kind.as_str() {
                "preflight" => self.preflight(&message["configuration"]),
                "turn" => self.turn(&message["turn"]),
                "complete" => self.complete(&message["request"]),
                "finalize" => Ok(self.finalize()),
                other => Err(format!(
                    "A take's body answers preflight, turn, complete and finalize; not {other:?}"
                )),
            };
            let (success, data) = match answer {
                Ok(data) => (true, data),
                Err(words) => (false, json!({"error": words})),
            };
            write(
                &mut output,
                json!({"type":"response","id":message["id"],"command":kind,"success":success,"data":data}),
            )?;
            if kind == "finalize" {
                return Ok(());
            }
        }
        self.finalize();
        Ok(())
    }

    fn preflight(&mut self, configuration: &Value) -> std::result::Result<Value, String> {
        if self.preflighted {
            return Err("A take's body is prepared once: its preflight already ran".into());
        }
        self.preflighted = true;
        let text = |key: &str| {
            configuration[key]
                .as_str()
                .filter(|value| !value.trim().is_empty())
        };
        let agent = text("agent").ok_or("The take's configuration names no agent")?;
        if agent != self.own.as_str() {
            return Err(format!(
                "This body is {}'s agent; the take's configuration names {agent}",
                self.own
            ));
        }
        let provider = text("provider").ok_or("The take's configuration names no provider")?;
        let model = text("model").ok_or("The take's configuration names no model")?;
        let world = text("world")
            .map(PathBuf::from)
            .filter(|world| world.is_absolute() && world.is_dir())
            .ok_or("The take's configuration names no World directory")?;
        let trace = text("trace_ref").ok_or("The take's configuration names no trace")?;
        let level = configuration.get("level").cloned().unwrap_or(Value::Null);
        let plan = level_of(&level, model)?;
        let recorded = EncounterService::session_agent(&self.home, &self.own)
            .map_err(|error| format!("The session's agent could not be read: {}", error.message()))?
            .ok_or(NEVER_OPENED)?;
        if recorded.provider != provider {
            return Err(format!(
                "{}'s agent opens on the encounter provider {}; the take's configuration names {provider}",
                self.own, recorded.provider
            ));
        }
        self.owner.call(&EncounterRequest::Health).map_err(|error| {
            format!(
                "The resident encounter owner does not answer at {}: {}. A take opens its agent there; start the owner and press Repeat again.",
                self.owner.socket.display(),
                error.message
            )
        })?;
        let (space, session) = take_session(&self.own, trace);
        // The take's NOW is composed before anything opens, and allocated once its
        // resident is open and its level applied.
        let takes = match configuration.get("takes") {
            None | Some(Value::Null) => vec![],
            Some(Value::Array(takes)) => takes
                .iter()
                .map(|take| take.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .ok_or("The take's configuration names its takes as text")?,
            Some(_) => return Err("The take's configuration names its takes as a list".into()),
        };
        let now_request = match crate::encounter_service::bound_task_now(&self.home, &self.own) {
            Err(error) => {
                return Err(format!(
                    "The session's task could not be read for the take's NOW: {}",
                    error.message()
                ))
            }
            Ok(None) => None,
            Ok(Some(task)) => Some(take_now_request(&task, trace, &takes, &session, &level)?),
        };
        // A set is carried into the World before the harness opens there, so it
        // finds the set at session start.
        let carried = match &plan {
            Level::Skillset { set, member } => {
                let (harness, prefix) =
                    EncounterService::provider_skill_root(&self.home, provider)?;
                Some(carry_set(
                    &self.home,
                    &world,
                    &harness,
                    &prefix,
                    set,
                    member.as_deref(),
                )?)
            }
            _ => None,
        };
        self.attach(&space, &session, trace).map_err(|error| {
            format!(
                "The take's session could not be prepared: {}",
                error.message()
            )
        })?;
        let receipt = self
            .owner
            .call(&EncounterRequest::Open {
                space,
                agent_session: session.clone(),
                provider: provider.to_owned(),
                cwd: world.clone(),
            })
            .map_err(|error| format!("The take's agent could not be opened: {}", error.message))?;
        let (Some(native), Some(generation)) = (
            receipt["native_session_id"].as_str(),
            receipt["connection_generation"].as_str(),
        ) else {
            return Err(
                "The encounter owner opened the take's agent without naming its native session"
                    .into(),
            );
        };
        self.opened = Some(Opened {
            session: session.clone(),
            native_session_id: native.to_owned(),
            generation: generation.to_owned(),
        });
        if let Level::ToolCalls(word) = &plan {
            self.tool_calls = Some(word.clone());
        }
        match self.apply(model, &plan) {
            Ok(mut applied) => {
                if let Some(carried) = &carried {
                    if let (Some(applied), Some(reading)) =
                        (applied.as_object_mut(), carried.reading.as_object())
                    {
                        applied.extend(reading.clone());
                    }
                }
                let now = match &now_request {
                    None => json!({"none": NO_TASK_NOW}),
                    Some(request) => {
                        match crate::encounter_service::allocate_central_now(request) {
                            Ok(kept) => {
                                let reading = json!({"now_ref": kept.allocation["now_ref"],
                                "parent_now_ref": kept.allocation["record"]["parent_now_ref"],
                                "task_ref": request.task_ref, "beside": request.source_refs});
                                self.now = Some(kept);
                                reading
                            }
                            Err(error) => {
                                self.finalize();
                                return Err(format!(
                                    "Central did not allocate the take's NOW: {}",
                                    central_words(&error)
                                ));
                            }
                        }
                    }
                };
                // The proposal keeps the take that ran under it, beside the takes
                // of its Repeat: the Test of §19.7.
                if let Some(member) = carried
                    .as_ref()
                    .and_then(|carried| carried.proposed.as_deref())
                {
                    let test = crate::skill_sources::RefinementTest {
                        trace: trace.to_owned(),
                        beside: takes
                            .iter()
                            .filter(|take| *take != trace)
                            .cloned()
                            .collect(),
                        now_ref: now["now_ref"].as_str().map(str::to_owned),
                    };
                    if let Err(error) = crate::skill_sources::record_test(&self.home, member, test)
                    {
                        self.finalize();
                        return Err(format!(
                            "The proposed revision of {member} did not keep this take as its test: {}",
                            error.message()
                        ));
                    }
                }
                let mut ready = json!({"ready": true, "provider": provider, "model": model,
                    "agent": agent, "agent_session": session.as_str(), "native_session_id": native,
                    "world": world, "applied": applied, "now": now});
                if !level.is_null() {
                    ready["level"] = level;
                }
                Ok(ready)
            }
            Err(words) => {
                self.finalize();
                Err(words)
            }
        }
    }

    /// Hold the configuration's model on the fresh resident, then apply the
    /// take's level, confirming each from the resident's own reading.
    fn apply(&self, model: &str, plan: &Level) -> std::result::Result<Value, String> {
        let opened = self.opened.as_ref().expect("opened before applying");
        let read_model = || {
            self.owner
                .call(&EncounterRequest::ModelRead {
                    agent_session: opened.session.clone(),
                })
                .map_err(|error| {
                    format!(
                        "The take's agent's model could not be read: {}",
                        error.message
                    )
                })
        };
        let select = |effort: Option<&str>| {
            self.owner
                .call(&EncounterRequest::ModelSelect {
                    agent_session: opened.session.clone(),
                    provider_model_id: model.to_owned(),
                    provider_reasoning_effort: effort.map(str::to_owned),
                    expected_native_session_id: Some(opened.native_session_id.clone()),
                })
                .map_err(|error| error.message)
        };
        let current = |read: &Value| {
            read["model_observation"]["current_model_id"]
                .as_str()
                .map(str::to_owned)
        };
        let mut read = read_model()?;
        if current(&read).as_deref() != Some(model) {
            select(None).map_err(|words| {
                format!("The take's agent could not hold the model {model}: {words}")
            })?;
            read = read_model()?;
            if current(&read).as_deref() != Some(model) {
                return Err(format!(
                    "The take's agent did not confirm the model {model}; it reads {}",
                    current(&read).unwrap_or_else(|| "none".into())
                ));
            }
        }
        let mut applied = json!({"model": model});
        match plan {
            // A set was carried into the World before the resident opened.
            Level::None | Level::Model | Level::Inherit | Level::Skillset { .. } => {}
            Level::Effort(effort) => {
                select(Some(effort)).map_err(|words| {
                    format!("The take's agent could not take the effort {effort}: {words}")
                })?;
                let read = read_model()?;
                let confirmed = crate::encounter_task_executor::selector_current(
                    &read["model_observation"]["reasoning_effort"],
                );
                if confirmed != Some(effort.as_str()) {
                    return Err(format!(
                        "The take's agent did not confirm the effort {effort}; it reads {}",
                        confirmed.unwrap_or("none")
                    ));
                }
                applied["reasoning_effort"] = json!(effort);
            }
            Level::ToolCalls(word) => {
                // Given on each turn, as the harness asks (`take`).
                applied["tool_calls"] = json!(word);
            }
        }
        Ok(applied)
    }

    fn take(&mut self, text: &str) -> std::result::Result<TurnAnswer, String> {
        let opened = self
            .opened
            .as_ref()
            .ok_or("The take's agent is not open: its preflight comes first")?;
        let baseline = self
            .owner
            .submit(&opened.session, text)
            .map_err(|(_, error)| {
                format!("The take's turn could not be submitted: {}", error.message)
            })?;
        let answer = self.owner.await_turn_answering(
            &opened.session,
            baseline,
            self.turn_timeout,
            TAKE_TURN,
            self.tool_calls.as_deref(),
        )?;
        self.turns += 1;
        Ok(answer)
    }

    fn turn(&mut self, turn: &Value) -> std::result::Result<Value, String> {
        let prompt = turn["prompt"]
            .as_str()
            .filter(|prompt| !prompt.trim().is_empty())
            .ok_or("A turn names its prompt")?;
        Ok(match self.take(prompt) {
            Ok(answer) => {
                json!({"status": "completed", "output": answer.said, "usage": usage_of(&answer)})
            }
            // A turn that ran and did not finish is the run's data, not a
            // refused operation.
            Err(words) => json!({"status": "failed", "output": "", "reason": words}),
        })
    }

    fn complete(&mut self, request: &Value) -> std::result::Result<Value, String> {
        let part = |key: &str| request[key].as_str().filter(|part| !part.trim().is_empty());
        let prompt = part("prompt").ok_or("A completion names its prompt")?;
        let text = match part("system") {
            Some(system) => format!("{system}\n\n{prompt}"),
            None => prompt.to_owned(),
        };
        let answer = self.take(&text)?;
        Ok(json!({"output": answer.said, "usage": usage_of(&answer)}))
    }

    /// Release the take's resident, once; its canonical history stays. Then
    /// close the take's NOW, which is the take's close.
    fn finalize(&mut self) -> Value {
        let mut answer = match self.opened.take() {
            None => {
                json!({"released": false, "turns": self.turns, "reason": "no resident was opened"})
            }
            Some(opened) => {
                let released = self.owner.call(&EncounterRequest::ReleaseNative {
                    agent_session: opened.session.clone(),
                    expected_native_session_id: opened.native_session_id.clone(),
                    expected_generation: opened.generation.clone(),
                });
                match released {
                    Ok(receipt) => json!({"released": true, "turns": self.turns,
                        "agent_session": opened.session.as_str(), "release": receipt}),
                    Err(error) => json!({"released": false, "turns": self.turns,
                        "agent_session": opened.session.as_str(), "reason": error.message}),
                }
            }
        };
        if let Some(now) = self.now.take() {
            let now_ref = now.allocation["now_ref"].clone();
            answer["now"] = match crate::encounter_service::close_central_now(&now) {
                Ok(Some(receipt)) => {
                    json!({"now_ref": now_ref, "lifecycle": receipt["record"]["lifecycle"]})
                }
                Ok(None) => json!({"now_ref": now_ref, "lifecycle": "active",
                    "reason": "Central had this take's NOW already, so the take did not close it"}),
                // Closing a NOW is a granted act (`central.now.lifecycle`): a body
                // that holds no grant leaves it active, in Central's words.
                Err(error) => json!({"now_ref": now_ref, "lifecycle": "active",
                    "reason": format!("Central did not close the take's NOW: {}", central_words(&error))}),
            };
        }
        answer
    }

    /// The take's SessionSpace and its session in it, prepared once.
    fn attach(&self, space: &SessionSpaceRef, session: &ResourceRef, trace: &str) -> Result<()> {
        let store = SessionSpaceApplicationStore::new(self.home.clone());
        let existing = match store.load(space) {
            Ok(state) => Some(state),
            Err(error) if error.code() == "session_space.not_found" => None,
            Err(error) => return Err(error),
        };
        if existing.is_none() {
            let set = trace.split(":trial:").next().unwrap_or(trace);
            let created = store
                .stage(
                    None,
                    SessionSpaceMutation::Create {
                        id: space.clone(),
                        label: Some(format!("takes of {} ({set})", self.own)),
                    },
                )
                .and_then(|preview| store.apply(&preview));
            if let Err(error) = created {
                store.load(space).map_err(|_| error)?;
            }
        }
        if !store.load(space)?.agent_sessions.contains_key(session) {
            let preview = store.stage(
                Some(space),
                SessionSpaceMutation::AttachAgentSession {
                    attachment: SessionSpaceAgentAttachmentIntent {
                        agent_session: session.clone(),
                        purpose: Some(format!("a take of {} ({trace})", self.own)),
                        provenance: vec![format!(
                            "prepared by `aikit session-space encounter-take-body` for {trace}; a fresh resident of {}'s agent, not an Agency grant",
                            self.own
                        )],
                    },
                },
            )?;
            store.apply(&preview)?;
        }
        Ok(())
    }
}

fn usage_of(answer: &TurnAnswer) -> Value {
    let provider = answer.usage.clone().unwrap_or(Value::Null);
    match answer.usage.as_ref().and_then(total_tokens) {
        Some(total) => json!({"total_tokens": total, "provider": provider}),
        None => json!({"provider": provider}),
    }
}

/// `aikit session-space encounter-take-body`: the protocol on this process's
/// stdin and stdout.
#[cfg(unix)]
pub fn serve(
    home: &AikitHome,
    own: &ResourceRef,
    socket: &Path,
    turn_timeout: Duration,
) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    TakeBody::new(home.clone(), own.clone(), socket.to_owned(), turn_timeout)
        .serve(stdin.lock(), stdout.lock())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> TakeHost {
        TakeHost {
            program: PathBuf::from("/opt/aikit/bin/aikit"),
            args_prefix: vec!["session-space".into()],
            environment: BTreeMap::from([("AIKIT_HOME".into(), "/home/me/.aikit".into())]),
            socket: PathBuf::from("/tmp/aikit-0123/owner.sock"),
        }
    }

    fn session() -> ResourceRef {
        ResourceRef::parse("agent-session/own").unwrap()
    }

    fn task() -> Value {
        json!({"task_ref": "task:fixtures", "name": "Run the fixtures"})
    }

    #[test]
    fn the_run_is_the_sessions_own_task_with_this_body_as_its_agent() {
        let agent = SessionAgent {
            provider: "take-peer".into(),
            model: Some("test/a".into()),
        };
        let run = take_run(&host(), &session(), &task(), Some(&agent));
        assert_eq!(
            run["run"]["task"],
            json!({"id": "task:fixtures", "prompt": "Run the fixtures", "successConditions": []})
        );
        let body = &run["run"]["body"];
        assert_eq!(body["protocol"], "agent-jsonl");
        assert_eq!(body["fixture_provider"], false);
        assert_eq!(
            body["configuration"],
            json!({"provider": "take-peer", "model": "test/a", "agent": "agent-session/own"})
        );
        assert_eq!(body["process"]["program"], "/opt/aikit/bin/aikit");
        assert_eq!(
            body["process"]["args"],
            json!([
                "session-space",
                "encounter-take-body",
                "--session",
                "agent-session/own",
                "--socket",
                "/tmp/aikit-0123/owner.sock"
            ])
        );
        assert_eq!(
            body["process"]["environment"]["AIKIT_HOME"],
            "/home/me/.aikit"
        );
        assert_eq!(body["session_timeout_ms"], SESSION_TIMEOUT_MS);
        assert!(run["run"]["max_steps"].as_u64().unwrap() >= 1);
        assert_eq!(run["basis"]["host_id"], "aikit-encounter-take-body");
        // Every constant Actuation holds a matched set to is stated, none left unknown.
        for held in [
            "host_revision",
            "benchmark_revision",
            "runner_revision",
            "review_contract_revision",
            "network_policy_digest",
        ] {
            assert!(run["basis"][held].is_string(), "{held}: {run}");
        }
        assert!(run.get("refused").is_none());
    }

    #[test]
    fn a_session_never_opened_or_with_no_model_has_no_run_and_says_why() {
        assert_eq!(
            take_run(&host(), &session(), &task(), None),
            json!({"refused": NEVER_OPENED})
        );
        let unobserved = SessionAgent {
            provider: "take-peer".into(),
            model: None,
        };
        assert_eq!(
            take_run(&host(), &session(), &task(), Some(&unobserved)),
            json!({"refused": NO_MODEL})
        );
    }

    #[test]
    fn a_task_bound_sessions_agent_is_the_tasks_own_not_its_launcher() {
        let task =
            json!({"launcher": {"id": "task-abc"}, "request": {"provider": {"id": "claude"}}});
        assert_eq!(
            agent_provider("task-abc", &task, |id| id == "claude"),
            "claude"
        );
        // Not configured: the launcher it opened on stands.
        assert_eq!(agent_provider("task-abc", &task, |_| false), "task-abc");
        // Opened on another provider, or bound to no task: that provider.
        assert_eq!(agent_provider("pi", &task, |_| true), "pi");
        assert_eq!(agent_provider("pi", &Value::Null, |_| true), "pi");
    }

    /// The session's prepared task as its preparation left it: its request and
    /// the NOW Central allocated for it.
    fn task_now() -> AllocatedCentralTask {
        serde_json::from_value(json!({
            "request": {"ctrl_bin": "/opt/central/ctrl", "central_root": "/home/me/Central", "project": "O-I",
                "task_ref": "task:fixtures", "purpose": "Run the fixtures", "workcell_ref": "workcell:env-2"},
            "allocation": {"now_ref": "central:now:project:o-i:0123"}}))
        .unwrap()
    }

    #[test]
    fn a_takes_now_hangs_from_its_tasks_and_names_the_takes_it_was_run_beside() {
        let takes: Vec<String> = (0..3).map(|n| format!("takes:own:5:trial:{n}")).collect();
        let take = ResourceRef::parse("agent-session/take-abc").unwrap();
        let level = json!({"variable": "effort", "value": "high"});
        let request =
            take_now_request(&task_now(), "takes:own:5:trial:1", &takes, &take, &level).unwrap();
        assert_eq!(
            request.task_ref.as_str(),
            "task:fixtures:take:takes:own:5:trial:1",
            "the take's own task, under its task's"
        );
        assert_eq!(
            request.parent_now_ref.as_ref().map(ResourceRef::as_str),
            Some("central:now:project:o-i:0123"),
            "a child of the task's NOW"
        );
        assert_eq!(
            request
                .source_refs
                .iter()
                .map(ResourceRef::as_str)
                .collect::<Vec<_>>(),
            [
                "task:fixtures:take:takes:own:5:trial:0",
                "task:fixtures:take:takes:own:5:trial:2"
            ],
            "the takes it was run beside, never itself"
        );
        assert_eq!(request.participant_refs, vec![take.clone()]);
        assert_eq!(request.ctrl_bin, PathBuf::from("/opt/central/ctrl"));
        assert_eq!(request.central_root, PathBuf::from("/home/me/Central"));
        assert_eq!(request.project.as_deref(), Some("O-I"));
        assert_eq!(
            request.workcell_ref.as_ref().map(ResourceRef::as_str),
            Some("workcell:env-2")
        );
        assert!(request.work_refs.is_empty());
        assert!(
            request.purpose.contains("effort high") && request.purpose.contains("2 other takes"),
            "{}",
            request.purpose
        );
        // A take its configuration does not place among the takes it names is refused.
        let refused = take_now_request(&task_now(), "takes:own:6:trial:0", &takes, &take, &level)
            .unwrap_err();
        assert!(refused.contains("is not among the takes"), "{refused}");
        // A take told no family is run beside none.
        let lone =
            take_now_request(&task_now(), "takes:own:5:trial:1", &[], &take, &Value::Null).unwrap();
        assert!(lone.source_refs.is_empty());
        // A task whose preparation names no NOW has none for a take's to hang from.
        let mut unallocated = task_now();
        unallocated.allocation = json!({});
        assert!(
            take_now_request(&unallocated, "takes:own:5:trial:1", &takes, &take, &level)
                .unwrap_err()
                .contains("names no NOW")
        );
    }

    /// A controlled test credential, never a person's: Central's own tests
    /// grant an agent `central.now.lifecycle` the same way.
    const TAKE_TOKEN: &str = "take-body-controlled-test-token-0123456789";

    /// A Central World on a temporary root, with the placement policy the
    /// session's task is prepared under (aikit-adapters caw_native_placement.rs)
    /// and an agent's grant to close a NOW.
    fn central_world(root: &Path) -> CentralTaskRequest {
        use sha2::Digest;
        for path in [
            "Control/user",
            "Control/relations",
            "Work/demo/src",
            "Work/demo/ProjectCentral",
        ] {
            std::fs::create_dir_all(root.join(path)).unwrap();
        }
        let path = "Control/user/placement.json";
        std::fs::write(root.join(path), json!({
            "schema":"central.work-placement-policy/v1", "scope_ref":"control:root",
            "writable":[{"path":"Work/demo", "class":"repository"}],
            "protected":["Work/demo/ProjectCentral"],
            "enforcement":"material-filesystem", "required_coverage":["file-content", "file-creation", "file-removal", "rename-link", "truncate", "descendant-processes"],
            "lease_seconds":300
        }).to_string()).unwrap();
        let authority = "Control/user/authority.json";
        std::fs::write(
            root.join(authority),
            json!({
                "schema":"central.native-action-authority/v1", "scope_ref":"control:root",
                "grants":[{"principal_ref":"agent:take-body", "actor_kind":"agent",
                    "token_sha256":format!("{:x}", sha2::Sha256::digest(TAKE_TOKEN.as_bytes())),
                    "scope_refs":["control:root"], "actions":["central.now.lifecycle"],
                    "expires_at_unix_seconds":u64::MAX}]
            })
            .to_string(),
        )
        .unwrap();
        let relation = |path: &str, role: &str| {
            json!({"ref":format!("central:source:control:root:{path}"),
            "path":path, "roles":[role], "provenance":"human-adopted", "standing":"architecture-contract",
            "treatment":"projectcentral-user", "recognition":"controlled-native-test-not-personal-adoption",
            "recorded_at_unix_seconds":1})
        };
        std::fs::write(root.join("Control/relations/source-relations.json"), json!({
            "schema":"central.control.ground-relations/v1", "project_id":"control:root",
            "relations":[relation(path, "work-placement-policy"), relation(authority, "native-action-authority")]
        }).to_string()).unwrap();
        CentralTaskRequest {
            ctrl_bin: PathBuf::from(
                std::env::var("AIKIT_CAW_CTRL_BIN").expect("exact source-built ctrl required"),
            ),
            central_root: root.to_owned(),
            project: None,
            task_ref: ResourceRef::parse("task:repeat-own").unwrap(),
            purpose: "The session's own task, which a Repeat takes".into(),
            participant_refs: vec![],
            source_refs: vec![],
            parent_now_ref: None,
            workcell_ref: None,
            work_refs: vec![],
        }
    }

    #[test]
    #[ignore = "requires exact source-built Central; mandatory in CAW workflow"]
    fn central_allocates_each_takes_now_under_its_tasks_and_closes_it_at_the_takes_end() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let task = crate::encounter_service::allocate_central_now(&central_world(&root)).unwrap();
        let takes: Vec<String> = (0..2).map(|n| format!("takes:own:7:trial:{n}")).collect();
        let level = json!({"variable": "tool_calls", "value": "session"});
        let mut kept = vec![];
        for trace in &takes {
            let (_, take) = take_session(&session(), trace);
            let request = take_now_request(&task, trace, &takes, &take, &level).unwrap();
            let now = crate::encounter_service::allocate_central_now(&request).unwrap();
            let record = &now.allocation["record"];
            assert_eq!(record["parent_now_ref"], task.allocation["now_ref"]);
            assert_eq!(record["horizon"], "child");
            assert_eq!(record["source_refs"], json!(request.source_refs));
            assert_eq!(record["participant_refs"], json!([take]));
            kept.push(now);
        }
        assert_ne!(kept[0].allocation["now_ref"], kept[1].allocation["now_ref"]);
        // Each names the other: a take's close is read against its sibling's.
        assert_eq!(
            kept[0].allocation["record"]["source_refs"],
            json!([kept[1].request.task_ref])
        );
        assert_eq!(
            kept[1].allocation["record"]["source_refs"],
            json!([kept[0].request.task_ref])
        );
        // Closing is a granted act: a body that holds no grant is refused, in Central's words.
        std::env::remove_var("CENTRAL_NATIVE_TOKEN");
        let refused = crate::encounter_service::close_central_now(&kept[0]).unwrap_err();
        assert!(
            central_words(&refused).contains("missing bounded CENTRAL_NATIVE_TOKEN"),
            "{refused:?}"
        );
        std::env::set_var("CENTRAL_NATIVE_TOKEN", TAKE_TOKEN);
        let closed = crate::encounter_service::close_central_now(&kept[0]);
        std::env::remove_var("CENTRAL_NATIVE_TOKEN");
        let closed = closed.unwrap().expect("the take's own NOW, new, is closed");
        assert_eq!(closed["record"]["lifecycle"], "closed");
        assert_eq!(closed["record"]["now_ref"], kept[0].allocation["now_ref"]);
    }

    #[test]
    fn a_take_lands_in_its_own_session_and_its_set_in_one_space() {
        let (space_a, take_a) = take_session(&session(), "takes:own:1:trial:0");
        let (space_b, take_b) = take_session(&session(), "takes:own:1:trial:1");
        let (space_c, _) = take_session(&session(), "takes:own:2:trial:0");
        assert_eq!(space_a, space_b, "one set, one space");
        assert_ne!(take_a, take_b, "one take, one session");
        assert_ne!(space_a, space_c, "another set, another space");
        let other = ResourceRef::parse("agent-session/other").unwrap();
        assert_ne!(take_session(&other, "takes:own:1:trial:0").1, take_a);
    }

    #[test]
    fn levels_are_read_as_the_body_applies_them_and_refused_where_it_cannot() {
        let level = |variable: &str, value: &str| json!({"variable": variable, "value": value});
        assert_eq!(level_of(&Value::Null, "m"), Ok(Level::None));
        assert_eq!(level_of(&level("model", "m"), "m"), Ok(Level::Model));
        assert!(level_of(&level("model", "n"), "m").is_err());
        assert_eq!(
            level_of(&level("effort", "high"), "m"),
            Ok(Level::Effort("high".into()))
        );
        assert_eq!(
            level_of(&level("effort", "inherit"), "m"),
            Ok(Level::Inherit)
        );
        for word in ["ask", "session", "always"] {
            assert_eq!(
                level_of(&level("tool_calls", word), "m"),
                Ok(Level::ToolCalls(word.into()))
            );
        }
        assert_eq!(
            level_of(&level("tool_calls", "inherit"), "m"),
            Ok(Level::Inherit)
        );
        assert!(level_of(&level("tool_calls", "reckless"), "m").is_err());
        let retired = level_of(&level("autonomy", "auto"), "m").unwrap_err();
        assert!(retired.contains("Autonomy is retired"), "{retired}");
        assert_eq!(
            level_of(&level("skillset", "docs"), "m"),
            Ok(Level::Skillset {
                set: "docs".into(),
                member: None
            })
        );
        assert_eq!(
            level_of(&level("skillset", "docs+skill/practice/alpha"), "m"),
            Ok(Level::Skillset {
                set: "docs".into(),
                member: Some("skill/practice/alpha".into())
            })
        );
        for value in ["+skill/practice/alpha", "docs+", " "] {
            assert!(level_of(&level("skillset", value), "m").is_err(), "{value}");
        }
        let refused = level_of(&level("budget", "x"), "m").unwrap_err();
        assert!(
            refused.contains("no owner operation on a fresh resident"),
            "{refused}"
        );
    }
}
