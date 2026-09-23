//! Refocus: the trace from the current operation back to ProjectCentral
//! ground (`aikit.refocus-reading/v1`), and its delivery through hooks.
//!
//! Delivery law (O:I `WORLD-INHABITATION-V1` §4, OpenRig `refocus.cjs`):
//!
//! * due at fresh occupancy (SessionStart `startup|clear`), after compaction
//!   (SessionStart `compact`, delivered directly), on a current-work transition,
//!   past a sustained-work threshold, and on explicit request;
//! * PreCompact / PostCompact / Stop only mark a Refocus pending — they print
//!   nothing; the next context-bearing event delivers it;
//! * delivery is recorded only after the harness output carrying it has been
//!   written ([`write_then_commit`]); a composed-but-unwritten Refocus stays
//!   due. Editing a file is not delivery;
//! * state is kept per (AgentSession, occupant generation): a new occupant
//!   generation starts its own baseline and never reads a predecessor's
//!   pending delivery.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aikit_adapters::runner::CommandRunner;
use aikit_core::hooks::{HookEvent, HookEventKind};
use aikit_core::inhabitation::{
    basis_digest, changed_sources, DeliveredRefocus, FacetState, NearbyWork, ReadingDepth,
    RefocusDecision, RefocusHop, RefocusPolicy, RefocusReading, RefocusSignal, RefocusState,
    RefocusTrigger, SessionSource, DEFAULT_SUSTAINED_PROMPTS, REFOCUS_READING_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::inhabitation::{
    self, current_work_digest, join, now_ms, pick, AikitReads, JoinInput, Joined, OwnerBins, Owners,
};

pub const SUSTAINED_PROMPTS_VAR: &str = "AIKIT_REFOCUS_PROMPTS";
pub const WORK_CHECK_SECS_VAR: &str = "AIKIT_REFOCUS_WORK_CHECK_SECS";
pub const HOOKS_VAR: &str = "AIKIT_INHABITATION_HOOKS";
const DEFAULT_WORK_CHECK_SECS: u64 = 60;
const OCCUPANT_SCHEMA: &str = "aikit.refocus-occupant/v1";
const MAX_TELOS_FILES: usize = 64;
const MAX_DIGEST_BYTES: u64 = 1024 * 1024;

// ---------------------------------------------------------------------------
// The reading
// ---------------------------------------------------------------------------

fn file_digest(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DIGEST_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

/// A digest over a folder's regular files (relative name + content digest),
/// bounded. Names, not bodies, travel.
fn folder_digest(root: &Path) -> Option<(String, usize)> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(4)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .take(MAX_TELOS_FILES)
    {
        let relative = entry.path().strip_prefix(root).ok()?.display().to_string();
        entries.push((relative, file_digest(entry.path()).unwrap_or_default()));
    }
    entries.sort();
    let bytes = serde_json::to_vec(&entries).ok()?;
    Some((
        format!("blake3:{}", blake3::hash(&bytes).to_hex()),
        entries.len(),
    ))
}

fn revision_of(value: &Value) -> Option<String> {
    for key in ["revision", "subjectRevision", "basisRevision"] {
        match value.get(key) {
            Some(Value::String(text)) if !text.is_empty() => return Some(text.clone()),
            Some(Value::Number(number)) => return Some(number.to_string()),
            _ => {}
        }
    }
    value.get("provenance").and_then(revision_of)
}

/// Build the Refocus reading from one joined inhabitation reading. Factory
/// hops are read here (Run, Journey); the Workflow unit rides the join.
pub fn build(
    owners: &Owners<'_>,
    joined: &Joined,
    trigger: RefocusTrigger,
    previous: Option<&DeliveredRefocus>,
    prompts: u64,
) -> RefocusReading {
    let reading = &joined.reading;
    let trail = &joined.trail;
    let f = &reading.facets;
    let identity = &reading.identity;
    let refs = &identity.current_work_refs;
    let state = trail.factory_state.clone();
    let mut chain = Vec::new();
    let mut basis = BTreeMap::new();

    // operation ← current work
    let work_source = "factory development current-work";
    match f.current_work.state {
        FacetState::Present => {
            let reference = refs
                .get("custody_ref")
                .or(refs.get("work_ref"))
                .cloned()
                .unwrap_or_default();
            let detail = trail.current_work.as_ref().map(|work| {
                format!(
                    "work {} [{}]",
                    refs.get("work_ref").map(String::as_str).unwrap_or("?"),
                    pick(&work["current"], &["state"]).unwrap_or_else(|| "in-progress".into())
                )
            });
            chain.push(RefocusHop::found(
                "operation",
                reference,
                identity.current_work_digest.clone(),
                detail,
                work_source,
            ));
        }
        _ => chain.push(RefocusHop::gap(
            "operation",
            f.current_work.line(),
            f.current_work.source.clone(),
        )),
    }

    // workflow unit
    let unit_source = "factory development workflow-unit";
    match (&trail.workflow_unit, refs.get("workflow_unit_ref")) {
        (Some(unit), _) => {
            let reference = pick(unit, &["workflowUnitRef", "workflow_unit_ref"])
                .or_else(|| refs.get("workflow_unit_ref").cloned())
                .unwrap_or_default();
            let revision = revision_of(unit);
            if let Some(revision) = &revision {
                basis.insert(reference.clone(), revision.clone());
            }
            chain.push(RefocusHop::found(
                "workflow-unit",
                reference,
                revision,
                pick(unit, &["developmentalConcern", "developmental_concern"]),
                unit_source,
            ));
        }
        (None, Some(unit)) => chain.push(RefocusHop::gap(
            "workflow-unit",
            format!(
                "{unit} is named by the current work but `factory development workflow-unit` did not answer"
            ),
            unit_source,
        )),
        (None, None) => chain.push(RefocusHop::gap(
            "workflow-unit",
            "the current work names no workflow unit",
            unit_source,
        )),
    }

    // attempt / Run
    let run_ref = refs.get("run_ref").cloned();
    let mut journey_ref = refs.get("journey_ref").cloned();
    match (&state, &run_ref) {
        (Some(state), Some(run)) => {
            let answer = owners.factory(&["development", "run", state, run]);
            match answer.ok() {
                Some(data) => {
                    let revision = revision_of(data);
                    if let Some(revision) = &revision {
                        basis.insert(run.clone(), revision.clone());
                    }
                    if journey_ref.is_none() {
                        let owning: Vec<String> = data["owningJourneyRefs"]
                            .as_array()
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        if owning.len() == 1 {
                            journey_ref = owning.into_iter().next();
                        }
                    }
                    chain.push(RefocusHop::found(
                        "run",
                        run.clone(),
                        revision,
                        pick(data, &["lifecycle"]).map(|lifecycle| {
                            format!(
                                "{lifecycle}{}",
                                pick(data, &["destination"])
                                    .map(|d| format!(": {d}"))
                                    .unwrap_or_default()
                            )
                        }),
                        "factory development run",
                    ));
                }
                None => chain.push(RefocusHop::gap(
                    "run",
                    answer.describe(),
                    "factory development run",
                )),
            }
        }
        (_, None) => chain.push(RefocusHop::gap(
            "run",
            "the current work names no Run or attempt",
            "factory development current-work",
        )),
        (None, Some(run)) => chain.push(RefocusHop::gap(
            "run",
            format!("{run} is named but no Factory state was located to read it from"),
            "factory project locate",
        )),
    }

    // Journey / Commission
    match (&state, &journey_ref) {
        (Some(state), Some(journey)) => {
            let answer = owners.factory(&["development", "journey", state, journey]);
            match answer.ok() {
                Some(data) => {
                    let revision = revision_of(data);
                    if let Some(revision) = &revision {
                        basis.insert(journey.clone(), revision.clone());
                    }
                    let commission = data.get("commission");
                    let detail = commission.map(|c| {
                        format!(
                            "commission {}{}",
                            pick(c, &["commission_ref", "commissionRef"])
                                .unwrap_or_else(|| "?".into()),
                            pick(c, &["purpose"])
                                .map(|p| format!(": {p}"))
                                .unwrap_or_default()
                        )
                    });
                    chain.push(RefocusHop::found(
                        "journey",
                        journey.clone(),
                        revision,
                        detail,
                        "factory development journey",
                    ));
                }
                None => chain.push(RefocusHop::gap(
                    "journey",
                    answer.describe(),
                    "factory development journey",
                )),
            }
        }
        _ => chain.push(RefocusHop::gap(
            "journey",
            "no Journey or Commission is named by the current work or its Run",
            "factory development run",
        )),
    }

    // Project intent ← ProjectCentral user/telos; ground ← project.json
    let project = trail.project_name.clone();
    match &trail.project_root {
        Some(root) => {
            let projectcentral = root.join("ProjectCentral");
            let telos = projectcentral.join("user/telos");
            let name = project.clone().unwrap_or_else(|| "?".into());
            match folder_digest(&telos).filter(|(_, count)| *count > 0) {
                Some((digest, count)) => {
                    let reference =
                        format!("central:source:project:{name}:ProjectCentral/user/telos");
                    basis.insert(reference.clone(), digest.clone());
                    chain.push(RefocusHop::found(
                        "intent",
                        reference,
                        Some(digest),
                        Some(format!("{count} telos source(s)")),
                        "ProjectCentral/user/telos",
                    ));
                }
                None => chain.push(RefocusHop::gap(
                    "intent",
                    format!(
                        "{} holds no telos source; name the Project's intent there",
                        telos.display()
                    ),
                    "ProjectCentral/user/telos",
                )),
            }
            let project_json = projectcentral.join("project.json");
            match file_digest(&project_json) {
                Some(digest) => {
                    let reference =
                        format!("central:source:project:{name}:ProjectCentral/project.json");
                    basis.insert(reference.clone(), digest.clone());
                    chain.push(RefocusHop::found(
                        "ground",
                        reference,
                        Some(digest),
                        None,
                        "ProjectCentral/project.json",
                    ));
                }
                None => chain.push(RefocusHop::gap(
                    "ground",
                    format!("{} is not readable", project_json.display()),
                    "ProjectCentral/project.json",
                )),
            }
        }
        None => {
            chain.push(RefocusHop::gap(
                "intent",
                "no Project World root resolved",
                "central.world.here",
            ));
            chain.push(RefocusHop::gap(
                "ground",
                "no Project World root resolved",
                "central.world.here",
            ));
        }
    }

    // Position and NOW revisions join the basis.
    if let (Some(position), Some(revision)) = (&identity.position_ref, &identity.position_revision)
    {
        basis.insert(position.clone(), revision.clone());
    }
    for (reference, revision) in [
        (&identity.root_now_ref, &identity.root_now_revision),
        (&identity.child_now_ref, &identity.child_now_revision),
    ] {
        if let (Some(reference), Some(revision)) = (reference, revision) {
            basis.insert(reference.clone(), revision.clone());
        }
    }
    let work_digest = match f.current_work.state {
        FacetState::Present | FacetState::Absent | FacetState::Ambiguous => {
            identity.current_work_digest.clone()
        }
        _ => None,
    };
    if let (Some(position), Some(digest)) = (&identity.position_ref, &work_digest) {
        basis.insert(format!("current-work:{position}"), digest.clone());
    }

    // Nearby work: occupied peers and what they carry.
    let mut nearby = Vec::new();
    for peer in &trail.peers {
        let Some(position) = pick(peer, &["position_ref"]) else {
            continue;
        };
        let occupancy = pick(peer, &["occupancy"]).unwrap_or_else(|| "unknown".into());
        if occupancy != "occupied" {
            continue;
        }
        let work = state.as_ref().map(|state| {
            let answer = owners.factory(&[
                "development",
                "current-work",
                state,
                "--position",
                &position,
            ]);
            match answer.ok() {
                Some(data) => match pick(data, &["outcome"]).as_deref() {
                    Some("one") => pick(&data["current"], &["work_ref", "workRef", "custody_ref"])
                        .unwrap_or_else(|| "one work node".into()),
                    Some("ambiguous") => "ambiguous current work".into(),
                    _ => "no current work".into(),
                },
                None => format!("unavailable ({})", answer.describe()),
            }
        });
        nearby.push(NearbyWork {
            position_ref: position,
            handle: pick(peer, &["handle"]),
            occupancy,
            work,
        });
    }

    let why = trigger.why(prompts);
    let changed = previous.map(|previous| changed_sources(&previous.basis, &basis));
    let return_target = f.return_destination.is_present().then(|| {
        format!(
            "{} (via {})",
            f.return_destination.summary.clone().unwrap_or_default(),
            f.return_destination.source
        )
    });
    let digest = basis_digest(&basis);
    RefocusReading {
        schema: REFOCUS_READING_SCHEMA.into(),
        trigger,
        why,
        position: f.position.summary.clone(),
        occupant_generation: identity.occupant_generation.clone(),
        chain,
        root_now: f.root_now.summary.clone(),
        child_now: f.child_now.summary.clone(),
        body: f.body.summary.clone(),
        nearby,
        changed,
        return_target,
        basis,
        basis_digest: digest,
        work_digest,
    }
}

// ---------------------------------------------------------------------------
// Per-occupant state
// ---------------------------------------------------------------------------

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "@._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A bounded, collision-stable file key: a lossy sanitisation or truncation
/// appends a short hash of the exact identity.
pub fn state_key(raw: &str) -> String {
    let bounded: String = sanitize(raw).chars().take(64).collect();
    if bounded == raw {
        return bounded;
    }
    format!(
        "{bounded}__{}",
        &blake3::hash(raw.as_bytes()).to_hex().as_str()[..8]
    )
}

/// Which occupant this AgentSession last entered as, with the owner
/// coordinates a per-prompt transition check needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccupantPointer {
    pub schema: String,
    pub agent_session: String,
    pub position_ref: String,
    pub occupant_generation: String,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub project_root: Option<PathBuf>,
    #[serde(default)]
    pub factory_state: Option<String>,
    pub at_unix_ms: u64,
}

pub struct RefocusStore {
    dir: PathBuf,
}

impl RefocusStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn for_home(home: &aikit_store::home::AikitHome) -> Self {
        Self::new(home.state().join("refocus"))
    }

    pub fn state_path(&self, session: &str, generation: &str) -> PathBuf {
        self.dir.join(format!(
            "{}__{}.json",
            state_key(session),
            state_key(generation)
        ))
    }

    fn pointer_path(&self, session: &str) -> PathBuf {
        self.dir
            .join(format!("{}.occupant.json", state_key(session)))
    }

    pub fn load(&self, session: &str, generation: &str) -> Option<RefocusState> {
        let raw = std::fs::read(self.state_path(session, generation)).ok()?;
        let state: RefocusState = serde_json::from_slice(&raw).ok()?;
        // A file keyed to another identity (a hash collision or a hand edit)
        // is never adopted.
        (state.agent_session == session && state.occupant_generation == generation).then_some(state)
    }

    pub fn save(&self, state: &RefocusState) -> std::io::Result<()> {
        write_atomic(
            &self.state_path(&state.agent_session, &state.occupant_generation),
            &serde_json::to_vec_pretty(state).map_err(std::io::Error::other)?,
        )
    }

    pub fn load_pointer(&self, session: &str) -> Option<OccupantPointer> {
        let raw = std::fs::read(self.pointer_path(session)).ok()?;
        let pointer: OccupantPointer = serde_json::from_slice(&raw).ok()?;
        (pointer.agent_session == session).then_some(pointer)
    }

    pub fn save_pointer(&self, pointer: &OccupantPointer) -> std::io::Result<()> {
        write_atomic(
            &self.pointer_path(&pointer.agent_session),
            &serde_json::to_vec_pretty(pointer).map_err(std::io::Error::other)?,
        )
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)
}

/// A composed Refocus waiting for proof of delivery.
#[derive(Debug, Clone)]
pub struct RefocusCommit {
    path: PathBuf,
    state: RefocusState,
    pub trigger: RefocusTrigger,
    pub text: String,
}

impl RefocusCommit {
    /// Whether a harness output document actually carries this Refocus as
    /// turn context (`hookSpecificOutput.additionalContext`).
    pub fn carried_by(&self, document: &str) -> bool {
        serde_json::from_str::<Value>(document)
            .ok()
            .and_then(|value| {
                value["hookSpecificOutput"]["additionalContext"]
                    .as_str()
                    .map(|context| context.contains(&self.text))
            })
            .unwrap_or(false)
    }

    /// Record the delivery. Call only after the carrying output was written.
    pub fn commit(&self) -> std::io::Result<()> {
        write_atomic(
            &self.path,
            &serde_json::to_vec_pretty(&self.state).map_err(std::io::Error::other)?,
        )
    }
}

/// Write the harness document, and only if the write (and flush) succeeded and
/// the document carries the Refocus, record the delivery. Returns whether a
/// delivery was recorded.
pub fn write_then_commit(
    out: &mut dyn Write,
    document: &str,
    commit: Option<&RefocusCommit>,
) -> std::io::Result<bool> {
    writeln!(out, "{document}")?;
    out.flush()?;
    match commit {
        Some(commit) if commit.carried_by(document) => {
            commit.commit()?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// Hook integration
// ---------------------------------------------------------------------------

/// The process facts the hook path reads, captured once so tests can supply
/// their own.
#[derive(Debug, Clone)]
pub struct HookEnv {
    pub enabled: bool,
    pub position: Option<String>,
    pub generation: Option<String>,
    pub aikit_session: Option<String>,
    pub sustained_prompts: u64,
    pub work_check_interval_ms: u64,
    /// Whether an Actuation occupancy store exists to match an AgentSession
    /// against (step 3 of the resolution chain). With none, SessionStart
    /// makes no owner call at all for an unstamped body.
    pub occupancy_store_present: bool,
    pub per_call: Duration,
    pub total: Duration,
}

impl HookEnv {
    pub fn from_process() -> Self {
        let var = |key: &str| {
            std::env::var(key)
                .ok()
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let store = var(inhabitation::OCCUPANCY_STORE_VAR)
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".actuation/occupancy"))
            });
        let store_present = store
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .is_some_and(|mut entries| entries.next().is_some());
        Self {
            enabled: !matches!(
                var(HOOKS_VAR)
                    .as_deref()
                    .map(str::to_ascii_lowercase)
                    .as_deref(),
                Some("0" | "off" | "false" | "no")
            ),
            position: var(inhabitation::POSITION_VAR),
            generation: var(inhabitation::GENERATION_VAR),
            aikit_session: var(inhabitation::AGENT_SESSION_VAR),
            sustained_prompts: var(SUSTAINED_PROMPTS_VAR)
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .unwrap_or(DEFAULT_SUSTAINED_PROMPTS),
            work_check_interval_ms: var(WORK_CHECK_SECS_VAR)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(DEFAULT_WORK_CHECK_SECS)
                .saturating_mul(1000),
            occupancy_store_present: store_present,
            per_call: Duration::from_secs(3),
            total: Duration::from_secs(8),
        }
    }
}

/// Everything the hook path needs, injectable.
pub struct HookContext<'a> {
    pub store: RefocusStore,
    pub env: HookEnv,
    pub runner: &'a (dyn CommandRunner + Sync),
    pub bins: OwnerBins,
    pub central_root: Option<PathBuf>,
}

/// What the hook path contributes to one dispatch.
#[derive(Debug, Default)]
pub struct HookInhabitation {
    /// Replaces the historical temporal floor at SessionStart.
    pub lean_entry: Option<String>,
    pub refocus: Option<RefocusCommit>,
    pub warnings: Vec<String>,
}

/// The harness AgentSession of a hook event: `session_id`, else the
/// transcript's basename. None means no delivery state can be kept.
pub fn hook_session(payload: &Value) -> Option<String> {
    pick(payload, &["session_id", "sessionId"]).or_else(|| {
        pick(payload, &["transcript_path", "transcriptPath"]).and_then(|path| {
            Path::new(&path)
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
    })
}

fn signal_for(event: &HookEvent) -> Option<RefocusSignal> {
    Some(match &event.kind {
        HookEventKind::SessionStart => RefocusSignal::SessionStart(SessionSource::parse(
            event.payload.get("source").and_then(Value::as_str),
        )),
        HookEventKind::UserPromptSubmit => RefocusSignal::UserPromptSubmit { work: None },
        HookEventKind::PreCompact => RefocusSignal::PreCompact,
        HookEventKind::Stop => RefocusSignal::Stop,
        HookEventKind::Other(name) if name.eq_ignore_ascii_case("PostCompact") => {
            RefocusSignal::PostCompact
        }
        _ => return None,
    })
}

fn make_owners<'a>(ctx: &HookContext<'a>) -> Owners<'a> {
    Owners::new(
        ctx.runner,
        ctx.bins.clone(),
        ctx.central_root.clone(),
        ctx.env.per_call,
        ctx.env.total,
    )
}

fn join_input(
    ctx: &HookContext<'_>,
    event: &HookEvent,
    session: &str,
    depth: ReadingDepth,
) -> JoinInput {
    let mut agent_sessions = vec![("hook session_id".to_owned(), session.to_owned())];
    if let Some(aikit) = &ctx.env.aikit_session {
        agent_sessions.push((inhabitation::AGENT_SESSION_VAR.to_owned(), aikit.clone()));
    }
    JoinInput {
        cwd: event
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        position_flag: None,
        env_position: ctx.env.position.clone(),
        env_generation: ctx.env.generation.clone(),
        agent_sessions,
        depth,
        allow_session_lookup: ctx.env.occupancy_store_present,
    }
}

fn compose_delivery(
    ctx: &HookContext<'_>,
    owners: &Owners<'_>,
    joined: &Joined,
    state: &RefocusState,
    trigger: RefocusTrigger,
    event_name: &str,
    omit_identity: bool,
) -> RefocusCommit {
    let reading = build(
        owners,
        joined,
        trigger,
        state.delivered.as_ref(),
        state.prompts_since_delivery,
    );
    let text = reading.render(omit_identity);
    let mut delivered = state.clone();
    delivered.record_delivered(
        trigger,
        event_name,
        reading.basis.clone(),
        reading.work_digest.clone(),
        now_ms(),
    );
    RefocusCommit {
        path: ctx
            .store
            .state_path(&state.agent_session, &state.occupant_generation),
        state: delivered,
        trigger,
        text,
    }
}

/// The inhabitation contribution to one hook event. Never fails: a missing
/// owner is a warning or a quieter reading, never a hook denial.
pub fn hook_prepare(ctx: &HookContext<'_>, event: &HookEvent) -> HookInhabitation {
    let mut out = HookInhabitation::default();
    if !ctx.env.enabled {
        return out;
    }
    let Some(signal) = signal_for(event) else {
        return out;
    };
    let Some(session) = hook_session(&event.payload) else {
        return out;
    };
    let policy = RefocusPolicy {
        sustained_prompts: ctx.env.sustained_prompts,
    };
    let now = now_ms();

    if let RefocusSignal::SessionStart(_) = signal {
        // Unstamped body and no occupancy store: nothing can resolve, so no
        // owner is asked and the historical floor stands unchanged.
        if ctx.env.position.is_none() && !ctx.env.occupancy_store_present {
            return out;
        }
        let owners = make_owners(ctx);
        // Unstamped: only an open tenure naming this AgentSession can resolve.
        // Ask Actuation that one question before running the whole join.
        if ctx.env.position.is_none() {
            let input = join_input(ctx, event, &session, ReadingDepth::Lean);
            let listed = owners.occupancy(&["list"]);
            let named = listed.ok().is_some_and(|data| {
                ["positions", "occupancies", "entries"]
                    .iter()
                    .filter_map(|key| data.get(*key).and_then(Value::as_array))
                    .flatten()
                    .filter_map(|entry| entry.get("current"))
                    .filter_map(|current| pick(current, &["agent_session_ref", "agentSessionRef"]))
                    .any(|session| input.agent_sessions.iter().any(|(_, id)| *id == session))
            });
            if !named {
                return out;
            }
        }
        let joined = join(
            &owners,
            &join_input(ctx, event, &session, ReadingDepth::Standard),
            &AikitReads::default(),
        );
        let reading = &joined.reading;
        let (Some(position), Some(generation)) = (
            reading.position_ref.clone(),
            reading.occupant_generation.clone(),
        ) else {
            if ctx.env.position.is_some() {
                out.warnings.push(format!(
                    "inhabitation: OI_POSITION_REF is stamped but no occupant generation resolves — {}",
                    reading.facets.position.line()
                ));
            }
            return out;
        };
        if reading.facets.occupancy.state == FacetState::Absent {
            out.warnings.push(format!(
                "inhabitation: {position} is not held by this body — {}",
                reading.facets.occupancy.line()
            ));
            return out;
        }
        out.lean_entry = Some(inhabitation::render_lean_entry(
            reading,
            joined.trail.project_name.as_deref(),
        ));
        let _ = ctx.store.save_pointer(&OccupantPointer {
            schema: OCCUPANT_SCHEMA.into(),
            agent_session: session.clone(),
            position_ref: position.clone(),
            occupant_generation: generation.clone(),
            project: joined.trail.project_name.clone(),
            project_root: joined.trail.project_root.clone(),
            factory_state: joined.trail.factory_state.clone(),
            at_unix_ms: now,
        });
        let mut state = ctx
            .store
            .load(&session, &generation)
            .unwrap_or_else(|| RefocusState::new(&session, &generation, &position, now));
        let decision = state.observe(&signal, policy, now);
        if let Err(error) = ctx.store.save(&state) {
            out.warnings
                .push(format!("refocus state could not be saved: {error}"));
        }
        if let RefocusDecision::Deliver(trigger) = decision {
            out.refocus = Some(compose_delivery(
                ctx,
                &owners,
                &joined,
                &state,
                trigger,
                signal.event_name(),
                true,
            ));
        }
        return out;
    }

    // Every other event rides the occupant this body entered as: the launch
    // stamp when present, else the pointer SessionStart left.
    let pointer = ctx.store.load_pointer(&session);
    let occupant = match (&ctx.env.position, &ctx.env.generation) {
        (Some(position), Some(generation)) => Some((position.clone(), generation.clone())),
        _ => pointer
            .as_ref()
            .map(|p| (p.position_ref.clone(), p.occupant_generation.clone())),
    };
    let Some((position, generation)) = occupant else {
        return out;
    };
    let mut state = ctx
        .store
        .load(&session, &generation)
        .unwrap_or_else(|| RefocusState::new(&session, &generation, &position, now));
    let mut signal = signal;
    if let RefocusSignal::UserPromptSubmit { work } = &mut signal {
        let due_check = state
            .last_work_check_unix_ms
            .is_none_or(|last| now.saturating_sub(last) >= ctx.env.work_check_interval_ms);
        let factory_state = pointer
            .as_ref()
            .filter(|p| p.occupant_generation == generation)
            .and_then(|p| p.factory_state.clone());
        if due_check {
            if let Some(factory_state) = factory_state {
                let owners = make_owners(ctx);
                let answer = owners.factory(&[
                    "development",
                    "current-work",
                    &factory_state,
                    "--position",
                    &position,
                ]);
                *work = Some(answer.ok().and_then(current_work_digest));
                state.last_work_check_unix_ms = Some(now);
            }
        }
    }
    let decision = state.observe(&signal, policy, now);
    if let Err(error) = ctx.store.save(&state) {
        out.warnings
            .push(format!("refocus state could not be saved: {error}"));
    }
    if let (RefocusDecision::Deliver(trigger), HookEventKind::UserPromptSubmit) =
        (decision, &event.kind)
    {
        let owners = make_owners(ctx);
        let mut input = join_input(ctx, event, &session, ReadingDepth::Standard);
        // The occupant is already known; resolve it directly.
        input.env_position = Some(position.clone());
        input.env_generation = Some(generation.clone());
        let joined = join(&owners, &input, &AikitReads::default());
        out.refocus = Some(compose_delivery(
            ctx,
            &owners,
            &joined,
            &state,
            trigger,
            "UserPromptSubmit",
            false,
        ));
    }
    out
}

/// The production hook path: process environment, AIKit home, real owners.
pub fn hook_prepare_process(
    home: &aikit_store::home::AikitHome,
    event: &HookEvent,
) -> HookInhabitation {
    // Tool-use and other events carry no inhabitation work: answer before
    // reading the environment or touching any file.
    if signal_for(event).is_none() {
        return HookInhabitation::default();
    }
    let env = HookEnv::from_process();
    if !env.enabled {
        return HookInhabitation::default();
    }
    let runner = aikit_adapters::runner::SystemRunner::new();
    let ctx = HookContext {
        store: RefocusStore::for_home(home),
        env,
        runner: &runner,
        bins: OwnerBins::from_env(),
        central_root: crate::temporal::central_root_enclosing(event.cwd.as_deref()),
    };
    hook_prepare(&ctx, event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inhabitation::tests::{owner_fixture, GENERATION, POSITION};
    use aikit_adapters::runner::ScriptedRunner;
    use serde_json::json;

    fn env(position: bool) -> HookEnv {
        HookEnv {
            enabled: true,
            position: position.then(|| POSITION.to_owned()),
            generation: position.then(|| GENERATION.to_owned()),
            aikit_session: None,
            sustained_prompts: 40,
            work_check_interval_ms: 0,
            occupancy_store_present: false,
            per_call: Duration::from_secs(5),
            total: Duration::from_secs(30),
        }
    }

    fn event(kind: HookEventKind, payload: Value) -> HookEvent {
        HookEvent::new("claude", kind, payload).in_cwd("/w/Work/O-I")
    }

    fn start(source: &str) -> HookEvent {
        event(
            HookEventKind::SessionStart,
            json!({"session_id": "sess-1", "source": source}),
        )
    }

    fn prompt() -> HookEvent {
        event(
            HookEventKind::UserPromptSubmit,
            json!({"session_id": "sess-1", "prompt": "go"}),
        )
    }

    fn ctx<'a>(dir: &Path, runner: &'a ScriptedRunner, env: HookEnv) -> HookContext<'a> {
        HookContext {
            store: RefocusStore::new(dir),
            env,
            runner,
            bins: OwnerBins::default(),
            central_root: Some(PathBuf::from("/w")),
        }
    }

    /// Deliver as the harness would: through the written stdout document.
    fn deliver(commit: &RefocusCommit, kind: &str) -> bool {
        let document = json!({"hookSpecificOutput": {"hookEventName": kind, "additionalContext": format!("other context\n\n{}", commit.text)}}).to_string();
        let mut out = Vec::new();
        write_then_commit(&mut out, &document, Some(commit)).unwrap()
    }

    #[test]
    fn fresh_start_with_an_occupancy_delivers_a_lean_entry_and_a_fresh_refocus() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx = ctx(dir.path(), &runner, env(true));
        let result = hook_prepare(&ctx, &start("startup"));
        let lean = result.lean_entry.expect("lean entry");
        assert!(lean.contains("World inhabitation"));
        let commit = result.refocus.expect("fresh refocus");
        assert_eq!(commit.trigger, RefocusTrigger::Fresh);
        assert!(commit.text.starts_with("REFOCUS (fresh occupancy"));
        assert!(commit.text.contains("workflow-unit"));
        assert!(
            !commit.text.contains("Position:"),
            "the lean entry carries identity"
        );
        // Composed is not delivered.
        let state = ctx.store.load("sess-1", GENERATION).unwrap();
        assert!(state.delivered.is_none());
        assert!(deliver(&commit, "SessionStart"));
        let state = ctx.store.load("sess-1", GENERATION).unwrap();
        assert_eq!(state.delivered.unwrap().trigger, RefocusTrigger::Fresh);
    }

    #[test]
    fn without_a_resolvable_occupancy_nothing_is_asked_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let runner = ScriptedRunner::new();
        let ctx = ctx(dir.path(), &runner, env(false));
        let result = hook_prepare(&ctx, &start("startup"));
        assert!(result.lean_entry.is_none());
        assert!(result.refocus.is_none());
        assert!(
            runner.calls().is_empty(),
            "no owner call for an unstamped body"
        );
        let result = hook_prepare(&ctx, &prompt());
        assert!(result.refocus.is_none());
        assert!(std::fs::read_dir(dir.path())
            .map(|mut d| d.next().is_none())
            .unwrap_or(true));
    }

    #[test]
    fn an_unstamped_session_start_asks_only_whether_a_tenure_names_it() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let mut unstamped = env(false);
        unstamped.occupancy_store_present = true;
        let first = ctx(dir.path(), &runner, unstamped);
        let result = hook_prepare(&first, &start("startup"));
        assert!(result.lean_entry.is_none());
        assert_eq!(runner.call_lines().len(), 1, "{:?}", runner.call_lines());
        assert!(runner.call_lines()[0].contains("occupancy list"));

        // A tenure naming this harness session resolves the occupant.
        let named = owner_fixture().on(
            "occupancy list",
            &json!({"positions": [{"position_ref": POSITION, "state": "occupied",
                "current": {"generation_ref": GENERATION, "agent_session_ref": "sess-1"}}]})
            .to_string(),
        );
        let mut unstamped = env(false);
        unstamped.occupancy_store_present = true;
        let second = ctx(dir.path(), &named, unstamped);
        let result = hook_prepare(&second, &start("startup"));
        assert!(result
            .lean_entry
            .unwrap()
            .contains("resolved by agent-session"));
        assert!(result.refocus.is_some());
    }

    #[test]
    fn an_unwritten_refocus_is_not_delivered_and_stays_due() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx = ctx(dir.path(), &runner, env(true));
        hook_prepare(
            &ctx,
            &event(HookEventKind::PreCompact, json!({"session_id": "sess-1"})),
        );
        let first = hook_prepare(&ctx, &prompt())
            .refocus
            .expect("pending compaction delivers");
        assert_eq!(first.trigger, RefocusTrigger::Compaction);
        // The harness document did not carry it (e.g. an unsupported event).
        let mut out = Vec::new();
        assert!(!write_then_commit(&mut out, "{}", Some(&first)).unwrap());
        let again = hook_prepare(&ctx, &prompt()).refocus.expect("still due");
        assert_eq!(again.trigger, RefocusTrigger::Compaction);
        assert!(deliver(&again, "UserPromptSubmit"));
        assert!(
            hook_prepare(&ctx, &prompt()).refocus.is_none(),
            "not every turn"
        );
    }

    #[test]
    fn a_failed_write_records_nothing() {
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("closed pipe"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx = ctx(dir.path(), &runner, env(true));
        let commit = hook_prepare(&ctx, &start("startup")).refocus.unwrap();
        let document =
            json!({"hookSpecificOutput": {"additionalContext": commit.text}}).to_string();
        assert!(write_then_commit(&mut Broken, &document, Some(&commit)).is_err());
        assert!(ctx
            .store
            .load("sess-1", GENERATION)
            .unwrap()
            .delivered
            .is_none());
    }

    #[test]
    fn precompact_and_stop_mark_pending_without_output_and_compact_start_delivers() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx = ctx(dir.path(), &runner, env(true));
        let pre = hook_prepare(
            &ctx,
            &event(HookEventKind::PreCompact, json!({"session_id": "sess-1"})),
        );
        assert!(pre.refocus.is_none() && pre.lean_entry.is_none());
        let stop = hook_prepare(
            &ctx,
            &event(HookEventKind::Stop, json!({"session_id": "sess-1"})),
        );
        assert!(stop.refocus.is_none());
        assert!(ctx
            .store
            .load("sess-1", GENERATION)
            .unwrap()
            .pending
            .is_some());
        let compact = hook_prepare(&ctx, &start("compact"));
        let commit = compact
            .refocus
            .expect("compaction delivered directly at SessionStart");
        assert_eq!(commit.trigger, RefocusTrigger::Compaction);
        assert!(commit.text.contains("just compacted"));
        assert!(deliver(&commit, "SessionStart"));
        assert!(ctx
            .store
            .load("sess-1", GENERATION)
            .unwrap()
            .pending
            .is_none());
    }

    #[test]
    fn postcompact_marks_pending_for_the_next_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx = ctx(dir.path(), &runner, env(true));
        let post = hook_prepare(
            &ctx,
            &event(
                HookEventKind::Other("PostCompact".into()),
                json!({"session_id": "sess-1"}),
            ),
        );
        assert!(post.refocus.is_none());
        let commit = hook_prepare(&ctx, &prompt()).refocus.unwrap();
        assert_eq!(commit.trigger, RefocusTrigger::Compaction);
    }

    #[test]
    fn sustained_threshold_delivers_once_then_waits_again() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let mut env = env(true);
        env.sustained_prompts = 3;
        env.work_check_interval_ms = u64::MAX;
        let ctx = ctx(dir.path(), &runner, env);
        assert!(hook_prepare(&ctx, &prompt()).refocus.is_none());
        assert!(hook_prepare(&ctx, &prompt()).refocus.is_none());
        let commit = hook_prepare(&ctx, &prompt()).refocus.expect("third prompt");
        assert_eq!(commit.trigger, RefocusTrigger::Sustained);
        assert!(commit.text.contains("3 prompts of sustained work"));
        assert!(deliver(&commit, "UserPromptSubmit"));
        assert!(hook_prepare(&ctx, &prompt()).refocus.is_none());
        assert!(hook_prepare(&ctx, &prompt()).refocus.is_none());
    }

    #[test]
    fn a_current_work_transition_delivers_and_names_the_changed_sources() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx1 = ctx(dir.path(), &runner, env(true));
        let fresh = hook_prepare(&ctx1, &start("startup")).refocus.unwrap();
        assert!(deliver(&fresh, "SessionStart"));
        assert!(
            hook_prepare(&ctx1, &prompt()).refocus.is_none(),
            "same work, quiet turn"
        );

        let moved = owner_fixture().on(
            "development current-work",
            &json!({"schema": "factory.current-work/v1", "outcome": "one",
                    "current": {"custody_ref": "factory:custody:c2", "work_ref": "work:w2", "run_ref": "run:r1", "journey_ref": "journey:j1", "workflow_unit_ref": "workflow-unit:u1", "state": "in-progress"},
                    "candidates": [], "considered": 1, "basis": "one"}).to_string(),
        );
        let ctx2 = ctx(dir.path(), &moved, env(true));
        let commit = hook_prepare(&ctx2, &prompt()).refocus.expect("transition");
        assert_eq!(commit.trigger, RefocusTrigger::Transition);
        assert!(commit.text.contains("current-work:"), "{}", commit.text);
        assert!(commit.text.contains("factory:custody:c2"));
    }

    #[test]
    fn a_new_occupant_generation_never_inherits_the_predecessor_pending() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture();
        let ctx1 = ctx(dir.path(), &runner, env(true));
        hook_prepare(
            &ctx1,
            &event(HookEventKind::PreCompact, json!({"session_id": "sess-1"})),
        );
        assert!(ctx1
            .store
            .load("sess-1", GENERATION)
            .unwrap()
            .pending
            .is_some());

        let mut successor = env(true);
        successor.generation =
            Some("actuation:generation:22222222-2222-4222-8222-222222222222".into());
        let ctx2 = ctx(dir.path(), &runner, successor);
        assert!(
            hook_prepare(&ctx2, &prompt()).refocus.is_none(),
            "the successor starts its own baseline"
        );
        let own = ctx2
            .store
            .load(
                "sess-1",
                "actuation:generation:22222222-2222-4222-8222-222222222222",
            )
            .unwrap();
        assert!(own.pending.is_none());
        assert_eq!(own.prompts_since_delivery, 1);
        assert!(
            ctx1.store
                .load("sess-1", GENERATION)
                .unwrap()
                .pending
                .is_some(),
            "testimony stays"
        );
    }

    #[test]
    fn a_superseded_stamp_gets_no_entry_and_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let runner = owner_fixture().failing(
            "occupancy verify",
            3,
            r#"{"code":"occupancy.superseded","fact":"superseded"}"#,
        );
        let ctx = ctx(dir.path(), &runner, env(true));
        let result = hook_prepare(&ctx, &start("startup"));
        assert!(result.lean_entry.is_none());
        assert!(result.refocus.is_none());
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("not held by this body")));
    }

    #[test]
    fn state_keys_are_bounded_and_collision_stable() {
        assert_eq!(state_key("sess-1"), "sess-1");
        let a = state_key("actuation:generation:a");
        let b = state_key("actuation_generation_a");
        assert_ne!(a, b);
        assert!(state_key(&"x".repeat(300)).len() < 80);
    }
}
