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
//!   `provider_reasoning_effort`; an autonomy word is placed on the resident's
//!   advertised permission modes as the task executor places it
//!   (`ModeSelect`). `inherit` applies nothing. It answers what was applied
//!   and confirmed — `{ready, provider, model, agent, level}` — or refuses in
//!   words. A skillset or a budget has no owner operation on a fresh resident
//!   yet, so a take that varies one is refused before anything opens.
//! - `turn {turn: {iteration, prompt}}` takes one turn of the resident and
//!   answers `{status, output, usage}`: the agent acts in its World in its
//!   own loop, and Actuation reads the stop condition between turns.
//! - `complete {request: {system, prompt}}` takes one turn with the projected
//!   request as its text and answers `{output, usage}` (a QL condition's
//!   turns).
//! - `finalize` releases the resident: an idle, exactly identified native
//!   body, its canonical history kept.
//!
//! A turn that asks for a tool's permission is cancelled: nobody answers the
//! provider's consent for the person in a take. Usage is the provider's own
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

/// The level a take varies, as the body applies it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Level {
    None,
    /// The model is the configuration's model: Actuation sets it so.
    Model,
    Effort(String),
    /// An autonomy word's rank among the executor's words.
    Autonomy(usize),
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
        _ if value == "inherit" && matches!(variable, "effort" | "autonomy") => Ok(Level::Inherit),
        "effort" => Ok(Level::Effort(value.to_owned())),
        "autonomy" => crate::encounter_task_executor::autonomy_word_rank(value)
            .map(Level::Autonomy)
            .ok_or_else(|| {
                format!(
                    "An autonomy level is one of inherit, {}; the take was given {value}",
                    crate::encounter_task_executor::AUTONOMY_WORDS.join(", ")
                )
            }),
        "skillset" | "budget" => Err(format!(
            "A take's body applies a model, an effort or an autonomy to the session's agent; a {variable} has no owner operation on a fresh resident yet, so this take cannot vary it"
        )),
        other => Err(format!("A take's body applies no {other} level")),
    }
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
        match self.apply(model, &plan) {
            Ok(applied) => {
                let mut ready = json!({"ready": true, "provider": provider, "model": model,
                    "agent": agent, "agent_session": session.as_str(), "native_session_id": native,
                    "world": world, "applied": applied});
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
            Level::None | Level::Model | Level::Inherit => {}
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
            Level::Autonomy(rank) => {
                let read = self
                    .owner
                    .call(&EncounterRequest::ModeRead {
                        agent_session: opened.session.clone(),
                    })
                    .map_err(|error| {
                        format!(
                            "The take's agent's modes could not be read: {}",
                            error.message
                        )
                    })?;
                let modes: Vec<&str> = read["mode_observation"]["available_modes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|mode| mode["id"].as_str())
                    .collect();
                let index = crate::encounter_task_executor::proportional_index(
                    *rank,
                    crate::encounter_task_executor::AUTONOMY_WORDS.len(),
                    modes.len(),
                )
                .ok_or(
                    "The take's agent advertises no permission modes to place its autonomy on",
                )?;
                let mode = modes[index];
                self.owner
                    .call(&EncounterRequest::ModeSelect {
                        agent_session: opened.session.clone(),
                        provider_mode_id: mode.to_owned(),
                        expected_native_session_id: Some(opened.native_session_id.clone()),
                    })
                    .map_err(|error| {
                        format!(
                            "The take's agent could not take the mode {mode}: {}",
                            error.message
                        )
                    })?;
                applied["mode"] = json!(mode);
                applied["mode_placement"] = json!("the autonomy word's rank placed evenly on the modes the resident advertises (the task executor's placement)");
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
        let answer =
            self.owner
                .await_turn(&opened.session, baseline, self.turn_timeout, TAKE_TURN)?;
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

    /// Release the take's resident, once; its canonical history stays.
    fn finalize(&mut self) -> Value {
        let Some(opened) = self.opened.take() else {
            return json!({"released": false, "turns": self.turns, "reason": "no resident was opened"});
        };
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
        assert_eq!(
            level_of(&level("autonomy", "auto"), "m"),
            Ok(Level::Autonomy(2))
        );
        assert!(level_of(&level("autonomy", "reckless"), "m").is_err());
        for variable in ["skillset", "budget"] {
            let refused = level_of(&level(variable, "x"), "m").unwrap_err();
            assert!(
                refused.contains("no owner operation on a fresh resident"),
                "{refused}"
            );
        }
    }
}
